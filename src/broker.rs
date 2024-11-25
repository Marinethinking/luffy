use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use config;
use rumqttd::local::LinkRx;
use rumqttd::{Broker, Config, Notification};
use tokio::sync::mpsc;
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
        let config_path = "config/rumqttd.toml";

        // Check if config file exists
        if !Path::new(config_path).exists() {
            error!("Config file not found at: {}", config_path);
            return Err(anyhow::anyhow!("Config file not found"));
        }

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

        let broker = Broker::new(config);
        info!("Broker instance created");

        let (mut link_tx, mut link_rx) = broker.link("singlenode").map_err(|e| {
            error!("Failed to create broker link: {}", e);
            e
        })?;

        info!("Broker links established");

        // Store broker instance
        self.broker = Some(broker);

        // Extract broker before spawning
        let mut broker = self.broker.take().unwrap();

        // Spawn the broker in a separate task
        tokio::spawn(async move {
            info!("Starting MQTT broker...");
            if let Err(e) = broker.start() {
                error!("Broker failed to start: {}", e);
                return Err(anyhow::anyhow!("Broker failed to start: {}", e));
            }
            Ok(())
        });

        // Sleep save the world, never remove this
        // Give the broker a moment to start up
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Subscribe to all topics
        if let Err(e) = link_tx.subscribe("#") {
            error!("Failed to subscribe to topics: {}", e);
            return Err(anyhow::anyhow!("Failed to subscribe to topics"));
        }

        info!("Successfully subscribed to all topics");

        let mut count = 0;
        self.running.store(true, Ordering::SeqCst);

        while self.running.load(Ordering::SeqCst) {
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

        info!("MQTT broker loop ended");
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
