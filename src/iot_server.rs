use std::env;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use rumqttc::QoS;
use serde_json::Value;
use tokio::fs;
use tokio::time::{self, Duration};
use tracing::{error, info};

use crate::aws_client::AwsClient;
use crate::util;
use crate::vehicle::Vehicle;

pub struct IotServer {
    vehicle: &'static Vehicle,
    mqtt_client: Option<rumqttc::AsyncClient>,
    running: Arc<AtomicBool>,
}

impl IotServer {
    pub async fn new() -> Self {
        let vehicle = Vehicle::instance().await;
        Self {
            vehicle,
            mqtt_client: None,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting IoT server...");
        // Connect to MQTT
        let mqtt_client = self.connect_mqtt().await?;
        self.mqtt_client = Some(mqtt_client.clone());

        // Subscribe to command topics
        mqtt_client
            .subscribe(
                &format!("{}/command/#", self.vehicle.device_id),
                QoS::AtLeastOnce,
            )
            .await?;

        self.running.store(true, Ordering::SeqCst);

        // Create telemetry interval
        let mut interval = tokio::time::interval(Duration::from_secs(4));

        // Run the telemetry loop
        while self.running.load(Ordering::SeqCst) {
            interval.tick().await;
            if let Err(e) = self.publish_telemetry().await {
                error!("Failed to publish telemetry: {}", e);
            }
        }

        Ok(())
    }

    async fn handle_message(&self, topic: &str, payload: &[u8]) -> Result<()> {
        let payload_str = String::from_utf8_lossy(payload);
        info!("Received message on {}: {}", topic, payload_str);

        match topic {
            t if t == format!("{}/command/mode", self.vehicle.device_id) => {
                let mode: String = serde_json::from_str(&payload_str)?;
                self.vehicle.update_flight_mode(mode)?;
            }
            t if t == format!("{}/command/arm", self.vehicle.device_id) => {
                let should_arm: bool = serde_json::from_str(&payload_str)?;
                if should_arm {
                    // self.vehicle.arm()?;
                } else {
                    // self.vehicle.disarm()?;
                }
            }
            // Add more command handlers as needed
            _ => {
                info!("Unhandled topic: {}", topic);
            }
        }
        Ok(())
    }

    pub async fn connect_mqtt(&self) -> Result<rumqttc::AsyncClient> {
        let config_dir = dirs::config_dir()
            .context("Failed to get config directory")?
            .join("luffy");

        let cert_path = config_dir.join("certificate.pem");
        let key_path = config_dir.join("private.key");

        // Read certificate and key files
        let cert_pem = fs::read(&cert_path).await?;
        let key_pem = fs::read(&key_path).await?;

        // AWS root certificate
        let aws_root_cert = include_bytes!("../certs/AmazonRootCA.pem");

        let device_id = util::get_device_mac();
        let aws_iot_endpoint = env::var("AWS_IOT_ENDPOINT")?;
        let aws_iot_port = env::var("AWS_IOT_PORT")?.parse::<u16>()?;
        let mut mqtt_options = rumqttc::MqttOptions::new(device_id, aws_iot_endpoint, aws_iot_port);

        mqtt_options
            .set_keep_alive(Duration::from_secs(30))
            .set_clean_session(true);

        // Use the Simple TLS configuration
        let transport = rumqttc::Transport::Tls(rumqttc::TlsConfiguration::Simple {
            ca: aws_root_cert.to_vec(),
            alpn: Some(vec!["mqtt".as_bytes().to_vec()]),
            client_auth: Some((cert_pem, key_pem)),
        });

        mqtt_options.set_transport(transport);
        let (client, mut eventloop) = rumqttc::AsyncClient::new(mqtt_options, 10);

        // Spawn event loop handler
        tokio::spawn(async move {
            loop {
                match eventloop.poll().await {
                    Ok(rumqttc::Event::Incoming(rumqttc::Packet::Publish(p))) => {
                        info!("Received message on topic {}: {:?}", p.topic, p.payload);
                        // handle_message(&p.topic, &p.payload);
                    }
                    Ok(event) => {
                        info!("MQTT Event: {:?}", event);
                    }
                    Err(e) => {
                        error!("MQTT Error: {:?}", e);
                    }
                }
            }
        });

        Ok(client)
    }

    async fn publish_telemetry(&self) -> anyhow::Result<()> {
        let state = self.vehicle.get_state_snapshot()?;
        let payload = serde_json::to_string(&state)?;

        if let Some(client) = &self.mqtt_client {
            client
                .publish(
                    &format!("{}/telemetry", self.vehicle.device_id),
                    rumqttc::QoS::AtLeastOnce,
                    false,
                    payload,
                )
                .await
                .context("Failed to publish telemetry")?;
        }
        Ok(())
    }

    pub async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}
