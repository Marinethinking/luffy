use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use config;
use rumqttd::{Broker, Config, Notification};
use tracing::{debug, error, info};

pub struct MqttBroker {
    broker: Option<Broker>,
    running: Arc<AtomicBool>,
    broker_handle: Option<tokio::task::JoinHandle<Result<()>>>,
}

impl MqttBroker {
    pub async fn new() -> Self {
        Self {
            broker: None,
            running: Arc::new(AtomicBool::new(false)),
            broker_handle: None,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Loading config from rumqttd.toml...");
        let config_paths = ["config/rumqttd.toml", "/etc/luffy/rumqttd.toml"];
        let config_path = config_paths
            .iter()
            .find(|&path| Path::new(path).exists())
            .ok_or_else(|| {
                error!(
                    "Config file not found in any of the locations: {:?}",
                    config_paths
                );
                anyhow::anyhow!("Config file not found")
            })?;

        info!(
            "Loading config from: {:?}",
            Path::new(config_path).canonicalize()?
        );

        let raw_config = config::Config::builder()
            .add_source(config::File::with_name(config_path))
            .build()
            .map_err(|e| {
                error!("Failed to build config: {}", e);
                e
            })?;

        let config: Config = raw_config.try_deserialize().map_err(|e| {
            error!("Failed to deserialize config: {}", e);
            e
        })?;

        info!("MQTT Config loaded successfully: {:?}", config);

        let mut broker = Broker::new(config);
        info!("Broker instance created");

        let (mut link_tx, mut link_rx) = broker.link("singlenode")?;
        info!("Broker links established");

        // Spawn the broker in a separate task and store its handle
        let broker_handle = tokio::spawn(async move {
            info!("Starting MQTT broker...");
            if let Err(e) = broker.start() {
                error!("Broker failed to start: {}", e);
                return Err(anyhow::anyhow!("Broker failed to start: {}", e));
            }
            Ok(())
        });
        self.broker_handle = Some(broker_handle);

        // Sleep to allow broker to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Subscribe to all topics
        if let Err(e) = link_tx.subscribe("#") {
            error!("Failed to subscribe to topics: {}", e);
            return Err(anyhow::anyhow!("Failed to subscribe to topics"));
        }

        info!("Successfully subscribed to all topics");

        // Spawn a separate task for the notification loop
        let running = self.running.clone();
        tokio::spawn(async move {
            let mut count = 0;
            while running.load(Ordering::SeqCst) {
                match link_rx.recv().unwrap() {
                    Some(notification) => match notification {
                        Notification::Forward(forward) => {
                            count += 1;
                            debug!(
                                "Topic = {:?}, Count = {}, Payload = {} bytes",
                                forward.publish.topic,
                                count,
                                forward.publish.payload.len()
                            );
                        }
                        v => debug!("Received notification: {:?}", v),
                    },
                    None => continue,
                }
            }
            info!("MQTT broker notification loop ended");
        });

        Ok(())
    }

    pub async fn stop(&mut self) {
        info!("Stopping MQTT broker...");
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.broker_handle.take() {
            handle.abort();
        }
        info!("MQTT broker stopped");
    }
}
