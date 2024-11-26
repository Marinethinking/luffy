use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use rumqttc::{AsyncClient, QoS};
use tokio::fs;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info};

use crate::aws_client::AwsClient;
use crate::config::CONFIG;
use crate::vehicle::Vehicle;
use crate::{util, vehicle};

pub struct IotServer {
    vehicle: &'static Vehicle,
    iot_client: Option<rumqttc::AsyncClient>,
    broker_client: Option<rumqttc::AsyncClient>,
    running: Arc<AtomicBool>,
}

impl IotServer {
    pub async fn new() -> Self {
        let vehicle = Vehicle::instance().await;
        Self {
            vehicle,
            iot_client: None,
            broker_client: None,
            running: Arc::new(AtomicBool::new(true)),
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        let mut handles = vec![];
        info!(
            "Starting IoT server... iot client enabled={}, broker client enabled={}",
            CONFIG.aws.iot.enabled, CONFIG.rumqttd.enabled
        );

        if CONFIG.aws.iot.enabled {
            handles.push(self.start_iot().await?);
        }

        if CONFIG.rumqttd.enabled {
            handles.push(self.start_broker().await?);
        }

        // Wait for both loops
        for handle in handles {
            if let Err(e) = handle.await {
                error!("Telemetry loop error: {}", e);
            }
        }
        Ok(())
    }

    pub async fn start_broker(&mut self) -> Result<JoinHandle<()>> {
        info!("Starting broker client...");
        let host = &CONFIG.rumqttd.host;
        let port = CONFIG.rumqttd.port;
        let mut mqtt_options = rumqttc::MqttOptions::new("luffy", host, port);
        mqtt_options
            .set_keep_alive(Duration::from_secs(30))
            .set_clean_session(true);

        let (client, mut connection) = rumqttc::AsyncClient::new(mqtt_options.clone(), 10);

        // Spawn a persistent connection handler
        let connection_handle = tokio::spawn(async move {
            info!("Starting broker connection event loop");
            loop {
                match connection.poll().await {
                    Ok(_) => debug!("Broker connection poll success"),
                    Err(e) => {
                        error!("Broker connection error: {:?}", e);
                        sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        });

        // Wait for connection to be ready
        for attempt in 1..=30 {
            match client.try_publish("luffy/connected", QoS::AtLeastOnce, false, "true") {
                Ok(_) => {
                    info!(
                        "Successfully connected to broker after {} attempts",
                        attempt
                    );
                    self.broker_client = Some(client.clone());
                    let vehicle = self.vehicle;
                    return Ok(tokio::spawn(async move {
                        Self::broker_telemetry(vehicle, client).await;
                    }));
                }
                Err(_) => {
                    debug!("Broker not ready, attempt {}/30", attempt);
                    sleep(Duration::from_secs(1)).await;
                }
            }
        }

        connection_handle.abort();
        Err(anyhow::anyhow!(
            "Failed to connect to broker after 30 attempts"
        ))
    }

    async fn broker_telemetry(vehicle: &'static Vehicle, client: AsyncClient) {
        let mut interval = tokio::time::interval(Duration::from_secs(4));
        loop {
            interval.tick().await;
            info!("Broker - Telemetry tick...");

            let state = match vehicle.get_state_snapshot() {
                Ok(state) => state,
                Err(e) => {
                    error!("Broker - Failed to get state snapshot: {}", e);
                    continue;
                }
            };

            let payload = match serde_json::to_string(&state) {
                Ok(payload) => payload,
                Err(e) => {
                    error!("Broker - Failed to serialize state: {}", e);
                    continue;
                }
            };

            let topic = format!("{}/telemetry", vehicle.device_id);
            debug!("Broker - Publishing telemetry: {}", payload);

            match client
                .publish(&topic, QoS::AtLeastOnce, false, payload)
                .await
            {
                Ok(_) => info!("Broker - Successfully published telemetry"),
                Err(e) => error!("Broker - Failed to publish telemetry: {}", e),
            }
        }
    }

    pub async fn start_iot(&mut self) -> Result<JoinHandle<()>> {
        info!("Starting IoT client...");
        if !self.is_registered() {
            let aws_client = AwsClient::instance().await;
            aws_client
                .register_device()
                .await
                .context("Failed to register device")?;
        }

        let mqtt_client = self.connect_iot().await?;
        self.iot_client = Some(mqtt_client.clone());

        mqtt_client
            .subscribe(
                format!("{}/command/#", self.vehicle.device_id),
                QoS::AtLeastOnce,
            )
            .await
            .context("Failed to subscribe")?;

        info!(
            "[IOT]Successfully subscribed to {}/command/#",
            self.vehicle.device_id
        );

        let vehicle = self.vehicle;
        let running = self.running.clone();

        let handle = tokio::spawn(async move {
            while running.load(Ordering::SeqCst) {
                Self::iot_telemetry(vehicle, mqtt_client.clone()).await;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
        Ok(handle)
    }

    async fn iot_telemetry(vehicle: &'static Vehicle, client: AsyncClient) {
        let mut interval = tokio::time::interval(Duration::from_secs(4));

        loop {
            interval.tick().await;
            info!("AWS - Telemetry tick...");

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

            let topic = format!("{}/telemetry", vehicle.device_id);
            debug!("AWS - Publishing telemetry: {}", payload);

            match client
                .publish(&topic, QoS::AtLeastOnce, false, payload)
                .await
            {
                Ok(_) => info!("AWS - Successfully published telemetry"),
                Err(e) => error!("AWS - Failed to publish telemetry: {}", e),
            }
        }
    }

    pub async fn connect_iot(&self) -> Result<rumqttc::AsyncClient> {
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
        let aws_iot_endpoint = &CONFIG.aws.iot.endpoint;
        let aws_iot_port = CONFIG.aws.iot.port;
        let client_id = format!("{}_{}", device_id, uuid::Uuid::new_v4());
        let mut mqtt_options = rumqttc::MqttOptions::new(client_id, aws_iot_endpoint, aws_iot_port);

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

        let vehicle = self.vehicle;
        // Spawn event loop handler
        tokio::spawn(async move {
            info!("Starting iot event loop...");
            loop {
                match eventloop.poll().await {
                    Ok(rumqttc::Event::Incoming(rumqttc::Packet::SubAck(_))) => {
                        info!("Subscription confirmed by iot");
                    }
                    Ok(rumqttc::Event::Incoming(rumqttc::Packet::ConnAck(_))) => {
                        info!("[IOT]Connected..... ");
                    }
                    Ok(rumqttc::Event::Incoming(rumqttc::Packet::Publish(p))) => {
                        debug!(
                            "[IOT]Received message - Topic: {}, Payload: {:?}",
                            p.topic,
                            String::from_utf8_lossy(&p.payload)
                        );
                        if let Err(e) = Self::handle_message(vehicle, &p.topic, &p.payload).await {
                            error!("[IOT]Failed to handle message: {}", e);
                        }
                    }
                    Ok(event) => {
                        // debug!("[IOT]Other MQTT Event: {:?}", event);
                    }
                    Err(e) => {
                        error!("[IOT]MQTT Error: {:?}", e);
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        });

        Ok(client)
    }

    async fn handle_message(vehicle: &Vehicle, topic: &str, payload: &[u8]) -> Result<()> {
        let payload_str = String::from_utf8_lossy(payload);
        info!("Received message on {}: {}", topic, payload_str);

        match topic {
            t if t == format!("{}/command/mode", vehicle.device_id) => {
                let payload_json: serde_json::Value = serde_json::from_str(&payload_str)?;
                info!("Payload: {}", payload_json);
                let mode = payload_json["mode"].as_str().unwrap_or("unknown");
                vehicle.update_flight_mode(mode.to_string())?;
            }
            t if t == format!("{}/command/arm", vehicle.device_id) => {
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

    pub async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    fn is_registered(&self) -> bool {
        let config_dir = dirs::config_dir()
            .context("Failed to get config directory")
            .unwrap()
            .join("luffy");

        let cert_path = config_dir.join("certificate.pem");
        cert_path.exists()
    }
}
