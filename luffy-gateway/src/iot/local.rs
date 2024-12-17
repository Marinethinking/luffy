use anyhow::Result;
use rumqttc::QoS;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info};

use crate::config::CONFIG;
use crate::vehicle::Vehicle;
use luffy_common::mqtt::MqttClient;

pub struct LocalIotClient {
    mqtt_client: Arc<Mutex<MqttClient>>,
    running: Arc<AtomicBool>,
    on_message: fn(topic: String, payload: String),
}

impl LocalIotClient {
    pub fn new(on_message: fn(topic: String, payload: String)) -> Self {
        Self {
            mqtt_client: Arc::new(Mutex::new(MqttClient::new(
                "gateway".to_string(),
                CONFIG.base.mqtt_host.clone(),
                CONFIG.base.mqtt_port,
                Some(on_message),
                CONFIG.base.health_report_interval,
                env!("CARGO_PKG_VERSION").to_string(),
            ))),
            running: Arc::new(AtomicBool::new(true)),
            on_message,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting local IoT client...");
        let mqtt_client = Arc::clone(&self.mqtt_client);
        let running = Arc::clone(&self.running);

        // Connect to broker
        {
            let mut client = mqtt_client.lock().await;
            client.connect().await?;
        }

        tokio::spawn(async move {
            if let Err(e) = Self::telemetry_loop(mqtt_client, running).await {
                error!("Telemetry loop error: {}", e);
            }
        });
        Ok(())
    }

    async fn telemetry_loop(
        mqtt_client: Arc<Mutex<MqttClient>>,
        running: Arc<AtomicBool>,
    ) -> Result<()> {
        let vehicle = Vehicle::instance().await;
        let local_interval = CONFIG.iot.local_interval;
        let mut interval = tokio::time::interval(Duration::from_secs(local_interval));
        let topic = format!("{}/telemetry", vehicle.vehicle_id);

        while running.load(Ordering::SeqCst) {
            interval.tick().await;

            let state = vehicle.get_state_snapshot().map_err(|e| {
                error!("Failed to get state snapshot: {}", e);
                e
            })?;

            let payload = serde_json::to_string(&state).map_err(|e| {
                error!("Failed to serialize state: {}", e);
                e
            })?;

            debug!("Publishing telemetry: {}", payload);

            let mqtt_client = mqtt_client.lock().await;
            mqtt_client.publish(&topic, &payload).await.map_err(|e| {
                error!("Failed to publish telemetry: {}", e);
                e
            })?;

            debug!("Successfully published telemetry");
        }

        Ok(())
    }

    pub async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    pub async fn subscribe(&self, topic: String) -> Result<()> {
        let mut mqtt_client = self.mqtt_client.lock().await;
        mqtt_client.subscribe(&topic).await?;
        Ok(())
    }
}
