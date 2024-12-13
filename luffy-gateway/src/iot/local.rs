use anyhow::Result;
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
}

impl Default for LocalIotClient {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalIotClient {
    pub fn new() -> Self {
        Self {
            mqtt_client: Arc::new(Mutex::new(MqttClient::new(
                "gateway".to_string(),
                CONFIG.base.mqtt_host.to_string(),
                CONFIG.base.mqtt_port,
                Some(Self::on_message),
                CONFIG.base.health_report_interval,
                env!("CARGO_PKG_VERSION").to_string(),
            ))),
            running: Arc::new(AtomicBool::new(true)),
        }
    }

    pub async fn start(&mut self) -> Result<JoinHandle<()>> {
        info!("Starting local IoT client...");

        let mut mqtt_client = self.mqtt_client.lock().await.clone();
        // Connect to broker
        let _connection_handle = mqtt_client.connect().await?;

        let running = self.running.clone();

        let vehicle = Vehicle::instance().await;
        mqtt_client
            .subscribe(&format!("{}/command/#", vehicle.vehicle_id))
            .await?;

        Ok(tokio::spawn(async move {
            Self::telemetry_loop(mqtt_client, running).await;
        }))
    }

    async fn telemetry_loop(mqtt_client: MqttClient, running: Arc<AtomicBool>) {
        let vehicle = Vehicle::instance().await;
        let local_interval = CONFIG.iot.local_interval;
        let mut interval = tokio::time::interval(Duration::from_secs(local_interval));

        while running.load(Ordering::SeqCst) {
            interval.tick().await;

            let state = match vehicle.get_state_snapshot() {
                Ok(state) => state,
                Err(e) => {
                    error!("Failed to get state snapshot: {}", e);
                    return;
                }
            };

            let payload = match serde_json::to_string(&state) {
                Ok(payload) => payload,
                Err(e) => {
                    error!("Failed to serialize state: {}", e);
                    return;
                }
            };

            let topic = format!("{}/telemetry", vehicle.vehicle_id);
            debug!("Publishing telemetry: {}", payload);

            if let Err(e) = mqtt_client.publish(&topic, &payload).await {
                error!("Failed to publish telemetry: {}", e);
            } else {
                debug!("Successfully published telemetry");
            }
        }
    }

    pub async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    fn on_message(topic: String, payload: String) {
        debug!(
            "Local IoT client received message: topic={}, payload={}",
            topic, payload
        );
    }
}
