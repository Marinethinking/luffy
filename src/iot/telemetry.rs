use anyhow::Result;
use rumqttc::{Client, QoS};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info};

use crate::vehicle::{Vehicle, VehicleState};

#[derive(Debug, Serialize, Deserialize)]
pub struct TelemetryMessage {
    pub device_id: String,
    pub timestamp: i64,
    pub state: VehicleState,
}

pub struct TelemetryPublisher {
    client: Client,
    vehicle: &'static Vehicle,
    topic: String,
    logger_prefix: String,
    interval: Duration,
    shutdown: tokio::sync::broadcast::Sender<()>,
}

impl TelemetryPublisher {
    pub fn new(
        client: Client,
        vehicle: &'static Vehicle,
        logger_prefix: String,
        interval: Duration,
    ) -> Self {
        info!("Creating telemetry publisher for {}", vehicle.device_id);
        let topic = format!("{}/telemetry", vehicle.device_id);
        let (shutdown, _) = tokio::sync::broadcast::channel(1);
        Self {
            client,
            vehicle,
            topic,
            logger_prefix,
            interval,
            shutdown,
        }
    }

    pub async fn publish_telemetry(&self) -> Result<()> {
        let state = self.vehicle.get_state_snapshot()?;
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

        let message = TelemetryMessage {
            device_id: self.vehicle.device_id.clone(),
            timestamp,
            state,
        };

        let payload = serde_json::to_string(&message)?;
        debug!("[{}] Publishing telemetry: {}", self.logger_prefix, payload);

        self.client
            .publish(&self.topic, QoS::AtLeastOnce, false, payload);

        Ok(())
    }

    pub async fn run(&self) {
        let mut rx = self.shutdown.subscribe();
        loop {
            tokio::select! {
                _ = rx.recv() => {
                    info!("[{}] Shutting down telemetry publisher", self.logger_prefix);
                    break;
                }
                _ = async {
                    if let Err(e) = self.publish_telemetry().await {
                        error!("[{}] Failed to publish telemetry: {}", self.logger_prefix, e);
                    }
                    sleep(self.interval).await;
                } => {}
            }
        }
    }

    pub fn stop(&self) {
        let _ = self.shutdown.send(());
    }
}
