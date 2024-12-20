use anyhow::{Context, Result};
use rumqttc::{AsyncClient, QoS};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::fs;

use tokio::time::Duration;
use tracing::{debug, error, info};

use crate::aws_client::AwsClient;
use crate::config::CONFIG;
use crate::vehicle::Vehicle;
use luffy_common::util;

pub struct RemoteIotClient {
    client: Option<AsyncClient>,
    running: Arc<AtomicBool>,
    on_message: fn(topic: String, payload: String),
}

impl RemoteIotClient {
    pub fn new(on_message: fn(topic: String, payload: String)) -> Self {
        Self {
            client: None,
            running: Arc::new(AtomicBool::new(true)),
            on_message,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting IoT client...");
        if !self.is_registered() {
            let aws_client = AwsClient::instance().await;
            aws_client
                .register_device()
                .await
                .context("Failed to register device")?;
            //TODO: remove aws credentials on production
        }

        let mqtt_client = self.connect().await?;
        self.client = Some(mqtt_client.clone());

        let running = self.running.clone();

        tokio::spawn(async move {
            while running.load(Ordering::SeqCst) {
                Self::telemetry_loop(mqtt_client.clone(), running.clone()).await;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
        Ok(())
    }

    async fn telemetry_loop(client: AsyncClient, running: Arc<AtomicBool>) {
        let remote_interval = CONFIG.iot.remote_interval;
        let mut interval = tokio::time::interval(Duration::from_secs(remote_interval));
        let vehicle = Vehicle::instance().await;
        while running.load(Ordering::SeqCst) {
            interval.tick().await;

            let state = match vehicle.get_state_snapshot() {
                Ok(state) => state,
                Err(e) => {
                    error!("AWS - Failed to get state snapshot: {}", e);
                    continue;
                }
            };

            let payload = match serde_json::to_string(&state) {
                Ok(payload) => payload,
                Err(e) => {
                    error!("AWS - Failed to serialize state: {}", e);
                    continue;
                }
            };

            let topic = format!("{}/telemetry", vehicle.vehicle_id);
            debug!("AWS - Publishing telemetry: {}", payload);

            match client
                .publish(&topic, QoS::AtLeastOnce, false, payload)
                .await
            {
                Ok(_) => debug!("AWS - Successfully published telemetry"),
                Err(e) => error!("AWS - Failed to publish telemetry: {}", e),
            }
        }
    }

    async fn connect(&self) -> Result<AsyncClient> {
        let config_dir = match std::env::var("RUST_ENV").as_deref() {
            Ok("dev") => dirs::config_dir()
                .context("Failed to get config directory")?
                .join("luffy"),
            _ => std::path::PathBuf::from("/etc/luffy"),
        };

        let cert_path = config_dir.join("certificate.pem");
        let key_path = config_dir.join("private.key");

        let cert_pem = fs::read(&cert_path).await?;
        let key_pem = fs::read(&key_path).await?;
        let aws_root_cert = include_bytes!("../../certs/AmazonRootCA.pem");

        let vehicle_id = util::get_vehicle_id(&CONFIG.base);
        let aws_iot_endpoint = &CONFIG.base.aws.iot.endpoint;
        let aws_iot_port = CONFIG.base.aws.iot.port;
        let client_id = format!("{}_{}", vehicle_id, uuid::Uuid::new_v4());
        let mut mqtt_options = rumqttc::MqttOptions::new(client_id, aws_iot_endpoint, aws_iot_port);

        mqtt_options
            .set_keep_alive(Duration::from_secs(30))
            .set_clean_session(true);

        let transport = rumqttc::Transport::Tls(rumqttc::TlsConfiguration::Simple {
            ca: aws_root_cert.to_vec(),
            alpn: Some(vec!["mqtt".as_bytes().to_vec()]),
            client_auth: Some((cert_pem, key_pem)),
        });

        mqtt_options.set_transport(transport);
        let (client, mut eventloop) = rumqttc::AsyncClient::new(mqtt_options, 10);
        let on_message = self.on_message;
        tokio::spawn(async move {
            debug!("Starting iot event loop...");
            loop {
                match eventloop.poll().await {
                    Ok(rumqttc::Event::Incoming(rumqttc::Packet::SubAck(_))) => {
                        debug!("Subscription confirmed by iot");
                    }
                    Ok(rumqttc::Event::Incoming(rumqttc::Packet::ConnAck(_))) => {
                        debug!("[IOT]Connected..... ");
                    }
                    Ok(rumqttc::Event::Incoming(rumqttc::Packet::Publish(p))) => {
                        debug!(
                            "[IOT]Received message - Topic: {}, Payload: {:?}",
                            p.topic,
                            String::from_utf8_lossy(&p.payload)
                        );
                        // if let Err(e) = Self::handle_message(&p.topic, &p.payload).await {
                        //     error!("[IOT]Failed to handle message: {}", e);
                        // }
                        let payload_str = String::from_utf8_lossy(&p.payload).to_string();
                        on_message(p.topic, payload_str);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        error!("[IOT]MQTT Error: {:?}", e);
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        });

        Ok(client)
    }

    async fn handle_message(topic: &str, payload: &[u8]) -> Result<()> {
        let vehicle = Vehicle::instance().await;
        let payload_str = String::from_utf8_lossy(payload);
        debug!("Received message on {}: {}", topic, payload_str);

        match topic {
            t if t == format!("{}/command/mode", vehicle.vehicle_id) => {
                let payload_json: serde_json::Value = serde_json::from_str(&payload_str)?;
                debug!("Payload: {}", payload_json);
                let mode = payload_json["mode"].as_str().unwrap_or("unknown");
                vehicle.update_flight_mode(mode.to_string())?;
            }
            t if t == format!("{}/command/arm", vehicle.vehicle_id) => {
                let should_arm: bool = serde_json::from_str(&payload_str)?;
                if should_arm {
                    // self.vehicle.arm()?;
                } else {
                    // self.vehicle.disarm()?;
                }
            }
            _ => {
                debug!("Unhandled topic: {}", topic);
            }
        }
        Ok(())
    }

    pub async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);

        if let Some(client) = &self.client {
            if let Err(e) = client
                .disconnect()
                .await
                .context("Failed to disconnect from broker")
            {
                error!("Failed to disconnect from AWS IoT broker: {}", e);
            }
        }
    }

    fn is_registered(&self) -> bool {
        let config_dir = match std::env::var("RUST_ENV").as_deref() {
            Ok("dev") => dirs::config_dir()
                .context("Failed to get config directory")
                .unwrap()
                .join("luffy"),
            _ => std::path::PathBuf::from("/etc/luffy"),
        };

        let cert_path = config_dir.join("certificate.pem");
        cert_path.exists()
    }

    pub async fn subscribe(&self, topic: String) -> Result<()> {
        if let Some(client) = &self.client {
            client.subscribe(topic, QoS::AtLeastOnce).await?
        }
        Ok(())
    }
}
