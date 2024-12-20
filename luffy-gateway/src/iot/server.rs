use anyhow::Result;
use tracing::{debug, error, info};

use crate::config::CONFIG;
use crate::iot::local::LocalIotHandler;
use crate::iot::remote::RemoteIotClient;
use crate::ota::version::VersionManager;
use crate::vehicle::Vehicle;

pub struct IotServer {
    remote_client: Option<RemoteIotClient>,
    local_client: Option<LocalIotHandler>,
}

impl IotServer {
    pub async fn new() -> Self {
        Self {
            remote_client: Some(RemoteIotClient::new(Self::on_message)),
            local_client: Some(LocalIotHandler::new(Self::on_message)),
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting IoT server...");

        if CONFIG.feature.remote_iot {
            if let Some(client) = &mut self.remote_client {
                client.start().await?;
            }
        }

        if CONFIG.feature.local_iot {
            if let Some(client) = &mut self.local_client {
                client.start().await?;
            }
        }

        self.subscribe_topics().await;
        Ok(())
    }

    pub async fn stop(&self) {
        if let Some(client) = &self.remote_client {
            client.stop().await;
        }
        if let Some(client) = &self.local_client {
            client.stop().await;
        }
    }

    async fn subscribe_topics(&self) {
        let vehicle = Vehicle::instance().await;
        let vehicle_id = vehicle.vehicle_id.clone();
        let topics = vec![
            format!("{}/ota/#", vehicle_id),
            format!("{}/command/#", vehicle_id),
        ];
        if let Some(client) = &self.remote_client {
            for topic in topics.clone() {
                client.subscribe(topic).await.unwrap();
            }
        }

        if let Some(client) = &self.local_client {
            for topic in topics.clone() {
                client.subscribe(topic).await.unwrap();
            }
        }
    }

    pub fn on_message(topic: String, payload: String) {
        tokio::spawn(async move {
            Self::handle_command(topic, payload).await.unwrap();
        });
    }

    async fn handle_command(topic: String, payload: String) -> Result<()> {
        info!("Received command: topic={}, payload={}", topic, payload);
        let vehicle = Vehicle::instance().await;
        let vehicle_id = vehicle.vehicle_id.clone();
        if topic.starts_with(&format!("{}/command/", vehicle_id)) {
            //TODO: handle command
        } else if topic.starts_with(&format!("{}/ota/request", vehicle_id)) {
            let version_manager = VersionManager::new();
            version_manager.check_and_apply_updates().await?;
        }
        Ok(())
    }
}
