use anyhow::{Context, Result};
use rumqttc::{AsyncClient, QoS};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info};

use crate::config::CONFIG;
use crate::vehicle::Vehicle;

pub struct LocalIotClient {
    client: Option<AsyncClient>,
    running: Arc<AtomicBool>,
}

impl LocalIotClient {
    pub fn new() -> Self {
        Self {
            client: None,
            running: Arc::new(AtomicBool::new(true)),
        }
    }

    pub async fn start(&mut self) -> Result<JoinHandle<()>> {
        info!("Starting broker client...");
        let host = &CONFIG.rumqttd.host;
        let port = CONFIG.rumqttd.port;
        let mut mqtt_options = rumqttc::MqttOptions::new("luffy", host, port);
        mqtt_options
            .set_keep_alive(Duration::from_secs(30))
            .set_clean_session(true);

        let (client, mut eventloop) = rumqttc::AsyncClient::new(mqtt_options.clone(), 10);

        // Spawn connection handler
        let connection_handle = tokio::spawn(async move {
            info!("Starting broker connection event loop");
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
                    }
                    Ok(_) => {}
                    Err(e) => {
                        error!("Broker connection error: {:?}", e);
                        sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        });

        // Wait for connection
        for attempt in 1..=30 {
            match client.try_publish("luffy/connected", QoS::AtLeastOnce, false, "true") {
                Ok(_) => {
                    debug!(
                        "Successfully connected to broker after {} attempts",
                        attempt
                    );
                    self.client = Some(client.clone());
                    let running = self.running.clone();
                    return Ok(tokio::spawn(async move {
                        Self::telemetry_loop(client, running).await;
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

    async fn telemetry_loop(client: AsyncClient, running: Arc<AtomicBool>) {
        let vehicle = Vehicle::instance().await;
        let local_interval = CONFIG.iot.telemetry.local_interval;
        let mut interval = tokio::time::interval(Duration::from_secs(local_interval));
        while running.load(Ordering::SeqCst) {
            interval.tick().await;

            let state = match vehicle.get_state_snapshot() {
                Ok(state) => state,
                Err(e) => {
                    error!("Broker - Failed to get state snapshot: {}", e);
                    return;
                }
            };

            let payload = match serde_json::to_string(&state) {
                Ok(payload) => payload,
                Err(e) => {
                    error!("Broker - Failed to serialize state: {}", e);
                    return;
                }
            };

            let topic = format!("{}/telemetry", vehicle.device_id);
            debug!("Broker - Publishing telemetry: {}", payload);

            match client
                .publish(&topic, QoS::AtLeastOnce, false, payload)
                .await
            {
                Ok(_) => debug!("Broker - Successfully published telemetry"),
                Err(e) => error!("Broker - Failed to publish telemetry: {}", e),
            }
        }
    }

    pub async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);

        if let Some(client) = &self.client {
            if let Err(e) = client
                .disconnect()
                .await
                .context("Failed to disconnect from broker")
            {
                error!("Failed to disconnect from broker: {}", e);
            }
        }
    }
}
