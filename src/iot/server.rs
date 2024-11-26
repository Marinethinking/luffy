use anyhow::Result;
use tracing::{error, info};

use crate::config::CONFIG;
use crate::iot::local::LocalIotClient;
use crate::iot::remote::RemoteIotClient;

pub struct IotServer {
    remote_client: Option<RemoteIotClient>,
    local_client: Option<LocalIotClient>,
}

impl IotServer {
    pub async fn new() -> Self {
        Self {
            remote_client: Some(RemoteIotClient::new()),
            local_client: Some(LocalIotClient::new()),
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        let mut handles = vec![];
        info!(
            "Starting IoT server... iot client enabled={}, broker client enabled={}",
            CONFIG.aws.iot.enabled, CONFIG.rumqttd.enabled
        );

        if CONFIG.aws.iot.enabled {
            if let Some(client) = &mut self.remote_client {
                handles.push(client.start().await?);
            }
        }

        if CONFIG.rumqttd.enabled {
            if let Some(client) = &mut self.local_client {
                handles.push(client.start().await?);
            }
        }

        // Wait for both loops
        for handle in handles {
            if let Err(e) = handle.await {
                error!("Telemetry loop error: {}", e);
            }
        }
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
}
