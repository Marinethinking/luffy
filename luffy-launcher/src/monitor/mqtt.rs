use crate::config::CFG;
use crate::monitor::service::{HealthReport, ServiceStatus, Services};
use crate::monitor::vehicle::VehicleState;
use anyhow::Result;

use luffy_common::iot::local::LocalIotClient;
use luffy_common::util::glob_match;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::OnceCell;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info};

// Add static instance
pub static MQTT_MONITOR: OnceCell<Arc<MqttMonitor>> = OnceCell::const_new();

// Add this struct to deserialize telemetry data
#[derive(Debug, Deserialize)]
struct TelemetryData {
    location: (f64, f64),
    yaw_degree: f32,
    battery_percentage: f32,
    armed: bool,
    flight_mode: String,
}

pub struct MqttMonitor {
    pub services: Arc<RwLock<Services>>,
    pub vehicle: Arc<RwLock<VehicleState>>,
    pub client: Arc<Mutex<LocalIotClient>>,
}

impl MqttMonitor {
    pub async fn instance() -> Arc<Self> {
        MQTT_MONITOR
            .get_or_init(|| async {
                let version = env!("CARGO_PKG_VERSION");

                Arc::new(Self {
                    services: Arc::new(RwLock::new(Services::new())),
                    vehicle: Arc::new(RwLock::new(VehicleState::default())),
                    client: Arc::new(Mutex::new(LocalIotClient::new(
                        "launcher".to_string(),
                        CFG.base.mqtt_host.to_string(),
                        CFG.base.mqtt_port,
                        None,
                        CFG.base.health_report_interval,
                        version.to_string(),
                    ))),
                })
            })
            .await
            .clone()
    }

    pub async fn start(&self) -> Result<()> {
        info!("Starting MQTT monitor");

        let mut client = self.client.lock().await;
        client.set_on_message(|topic, payload| {
            tokio::spawn(Self::handle_message(topic, payload));
        });

        client.connect().await?;
        let timeout = Duration::from_secs(30);
        let start = std::time::Instant::now();
        loop {
            if client.connected {
                break;
            }
            if start.elapsed() > timeout {
                anyhow::bail!("Timeout waiting for MQTT connection");
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        info!("MQTT connected");

        client.subscribe("luffy/+/health").await?;
        client.subscribe("+/telemetry").await?;
        Ok(())
    }

    async fn handle_message(topic: String, payload: String) {
        debug!(
            "Monitor received message: topic={}, payload={}",
            topic, payload
        );
        let instance = MQTT_MONITOR.get().unwrap();

        if glob_match("luffy/+/health", &topic) {
            let service_name = topic.split('/').nth(1).unwrap_or("unknown");

            if let Ok(health) = serde_json::from_str::<HealthReport>(&payload) {
                let mut services = instance.services.write().await;
                let version = health.version.clone();
                services.set_service(
                    service_name,
                    Some(ServiceStatus::Running),
                    Some(version),
                    None,
                );
                debug!(
                    "Service {} is running with version {}",
                    service_name, health.version
                );
            } else {
                debug!("Failed to parse health report: {}", payload);
            }
        } else if glob_match("+/telemetry", &topic) {
            // Handle telemetry data
            if let Ok(telemetry) = serde_json::from_str::<TelemetryData>(&payload) {
                let mut vehicle = instance.vehicle.write().await;
                vehicle.location = telemetry.location;
                vehicle.yaw_degree = telemetry.yaw_degree;
                vehicle.battery_percentage = telemetry.battery_percentage;
                vehicle.armed = telemetry.armed;
                vehicle.flight_mode = telemetry.flight_mode;
                debug!("Updated vehicle state from telemetry");
            } else {
                debug!("Failed to parse telemetry data: {}", payload);
            }
        }
    }

    pub async fn get_services_snapshot(&self) -> Result<Services> {
        let services = self.services.read().await;
        debug!("Services: {:?}", services);
        Ok(services.clone())
    }

    pub async fn get_vehicle_snapshot(&self) -> Result<VehicleState> {
        let vehicle = self.vehicle.read().await;
        Ok(vehicle.clone())
    }
}
