use std::time::Duration;

use anyhow::Result;
use luffy_common::mqtt::MqttClient;
use luffy_common::util::glob_match;
use tracing::{debug, info};

pub struct MqttMonitor {
    client: MqttClient,
}

impl MqttMonitor {
    pub fn new(name: String, host: String, port: u16) -> Self {
        Self {
            client: MqttClient::new(name, host, port, None),
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        self.client.set_on_message(|topic, payload| {
            Self::handle_message(topic, payload);
        });

        self.client.connect().await?;
        let timeout = Duration::from_secs(30);
        let start = std::time::Instant::now();
        loop {
            if self.client.connected {
                break;
            }
            if start.elapsed() > timeout {
                anyhow::bail!("Timeout waiting for MQTT connection");
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        info!("MQTT connected");

        self.client.subscribe("luffy/+/health").await?;
        Ok(())
    }

    fn handle_message(topic: String, payload: String) {
        info!(
            "Monitor received message: topic={}, payload={}",
            topic, payload
        );
        let pattern = glob_match("luffy/+/health", &topic);
        if pattern {
            info!("Monitor received health message: {}", payload);
        }
    }
}
