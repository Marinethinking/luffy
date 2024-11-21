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

    pub async fn start(&self) -> Result<()> {
        // Connect to MQTT
        let mqtt_client = self.connect_mqtt().await?;

        // Subscribe to command topics
        mqtt_client
            .subscribe("vehicle/command/#", QoS::AtLeastOnce)
            .await?;

        self.running.store(true, Ordering::SeqCst);

        Ok(())
    }

    async fn handle_message(&self, topic: &str, payload: &[u8]) -> Result<()> {
        let payload_str = String::from_utf8_lossy(payload);
        info!("Received message on {}: {}", topic, payload_str);

        match topic {
            "vehicle/command/mode" => {
                let mode: String = serde_json::from_str(&payload_str)?;
                self.vehicle.update_flight_mode(mode)?;
            }
            "vehicle/command/arm" => {
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

        // Use the Simple TLS configuration
        let transport = rumqttc::Transport::Tls(rumqttc::TlsConfiguration::Simple {
            ca: aws_root_cert.to_vec(),
            alpn: None,
            client_auth: Some((cert_pem, key_pem)),
        });

        mqtt_options.set_transport(transport);
        let (client, mut eventloop) = rumqttc::AsyncClient::new(mqtt_options, 10);

        // Spawn event loop handler
        tokio::spawn(async move {
            loop {
                match eventloop.poll().await {
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
                    "vehicle/telemetry",
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
