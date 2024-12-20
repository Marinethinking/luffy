use crate::aws::AwsClient;
use anyhow::{Context, Result};
use derivative::Derivative;
use rumqttc::{AsyncClient, QoS};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::fs;
use tokio::time::Duration;
use tracing::{debug, error, info};
use uuid::Uuid;

#[derive(Derivative)]
#[derivative(Debug)]
pub struct RemoteIotClient {
    client: Option<AsyncClient>,
    vehicle_id: String,
    running: Arc<AtomicBool>,
    on_message: fn(topic: String, payload: String),
    aws_iot_endpoint: String,
    aws_iot_port: u16,
}

impl RemoteIotClient {
    pub fn new(
        on_message: fn(topic: String, payload: String),
        vehicle_id: String,
        aws_iot_endpoint: String,
        aws_iot_port: u16,
    ) -> Self {
        Self {
            client: None,
            vehicle_id,
            running: Arc::new(AtomicBool::new(true)),
            on_message,
            aws_iot_endpoint,
            aws_iot_port,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting IoT client...");
        if !self.is_registered() {
            let aws_client = AwsClient::instance().await;
            aws_client.register_device().await?;
        }

        let mqtt_client = self.connect().await?;
        self.client = Some(mqtt_client.clone());

        Ok(())
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

        let client_id = format!("{}_{}", self.vehicle_id, Uuid::new_v4());
        let mut mqtt_options =
            rumqttc::MqttOptions::new(client_id, self.aws_iot_endpoint.clone(), self.aws_iot_port);

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

    pub async fn subscribe(&self, topic: &str) -> Result<()> {
        if let Some(client) = &self.client {
            client.subscribe(topic, QoS::AtLeastOnce).await?;
        }
        Ok(())
    }

    pub async fn publish(&self, topic: &str, payload: &str) -> Result<()> {
        if let Some(client) = &self.client {
            client
                .publish(topic, QoS::AtLeastOnce, false, payload)
                .await?;
        }
        Ok(())
    }

    pub async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(client) = &self.client {
            if let Err(e) = client.disconnect().await {
                error!("Failed to disconnect from AWS IoT broker: {}", e);
            }
        }
    }
}
