use anyhow::Result;
use rumqttc::{AsyncClient, Event, Packet, QoS};
use serde_json::json;
use serde_json::Value as JsonValue;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info};

#[derive(Clone)]
pub struct MqttClient {
    host: String,
    port: u16,
    name: String,
    on_message: Option<fn(topic: String, payload: String)>,
    pub connected: bool,
    client: Option<AsyncClient>,
    health_report_interval: u64,
    version: String,
}

impl Default for MqttClient {
    fn default() -> Self {
        Self {
            name: "mqtt-client".to_string(),
            host: "localhost".to_string(),
            port: 9183,
            on_message: None,
            connected: false,
            client: None,
            health_report_interval: 60,
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

impl MqttClient {
    pub fn new(
        name: String,
        host: String,
        port: u16,
        on_message: Option<fn(topic: String, payload: String)>,
        health_report_interval: u64,
        version: String,
    ) -> Self {
        Self {
            name,
            host,
            port,
            on_message,
            connected: false,
            client: None,
            health_report_interval,
            version,
        }
    }

    pub async fn publish(&self, topic: &str, payload: &str) -> Result<()> {
        if let Some(client) = &self.client {
            client
                .publish(topic, QoS::AtLeastOnce, false, payload)
                .await?;
        }
        Ok(())
    }

    pub async fn subscribe(&self, topic: &str) -> Result<()> {
        info!("📥 Attempting to subscribe to topic: {}", topic);
        if let Some(client) = &self.client {
            match client.subscribe(topic, QoS::AtLeastOnce).await {
                Ok(_) => info!("✅ Successfully subscribed to {}", topic),
                Err(e) => error!("❌ Failed to subscribe to {}: {:?}", topic, e),
            }
        } else {
            error!("❌ Cannot subscribe: client not connected");
        }
        Ok(())
    }

    pub async fn connect(&mut self) -> Result<JoinHandle<()>> {
        info!("Starting broker client {}...", self.name);

        let mut mqtt_options =
            rumqttc::MqttOptions::new(self.name.clone(), self.host.clone(), self.port);
        mqtt_options
            .set_keep_alive(Duration::from_secs(30))
            .set_clean_session(true);

        info!("Connecting to MQTT broker at {}:{}", self.host, self.port);
        let (client, mut eventloop) = rumqttc::AsyncClient::new(mqtt_options, 10);
        self.client = Some(client.clone());

        let on_message = self.on_message;
        let name = self.name.clone();

        // Spawn the connection handling task
        let connection_handle = tokio::spawn(async move {
            info!("🚀 Starting broker connection event loop for {}", name);
            let mut connection_established = false;

            loop {
                match eventloop.poll().await {
                    Ok(Event::Incoming(Packet::SubAck(ack))) => {
                        info!("✅ Subscription confirmed: {:?}", ack);
                    }
                    Ok(Event::Incoming(Packet::ConnAck(ack))) => {
                        connection_established = true;
                        info!("🔗 Connected to broker: {:?}", ack);
                    }
                    Ok(Event::Incoming(Packet::Publish(p))) => {
                        debug!(
                            "📨 Received message - Topic: {}, Payload: {:?}",
                            p.topic,
                            String::from_utf8_lossy(&p.payload)
                        );
                        if let Some(callback) = on_message {
                            callback(p.topic, String::from_utf8_lossy(&p.payload).to_string());
                        } else {
                            info!("📝 No message handler set");
                        }
                    }
                    Ok(event) => {
                        debug!("📝 Other MQTT event received: {:?}", event);
                    }
                    Err(e) => {
                        error!("❌ Broker connection error: {:?}", e);
                        if connection_established {
                            error!("📡 Connection lost, attempting to reconnect...");
                            connection_established = false;
                        }
                        sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        });

        // Wait for connection
        info!("Attempting to establish initial connection...");
        for attempt in 1..=30 {
            info!("Connection attempt {}/30", attempt);
            match client.try_publish(
                format!("/luffy/{}/connected", self.name),
                QoS::AtLeastOnce,
                false,
                "true",
            ) {
                Ok(_) => {
                    info!(
                        "✅ Successfully connected to broker after {} attempts",
                        attempt
                    );
                    self.connected = true;
                    let client = self.client.clone();
                    let name = self.name.clone();
                    let interval = self.health_report_interval;
                    let health_report_payload = json!({
                        "version": self.version
                    })
                    .to_string();
                    // Spawn health report task
                    tokio::spawn(async move {
                        info!("🏥 Starting health report task for {}", name);
                        if let Err(e) =
                            Self::health_report_task(client, name, interval, health_report_payload)
                                .await
                        {
                            error!("❌ Health report task failed: {:?}", e);
                        }
                    });

                    return Ok(connection_handle);
                }
                Err(e) => {
                    debug!("Broker not ready, attempt {}/30. Error: {:?}", attempt, e);
                    sleep(Duration::from_secs(1)).await;
                }
            }
        }

        // If we get here, connection failed
        error!("❌ Failed to connect after 30 attempts, aborting connection handle");
        connection_handle.abort();
        Err(anyhow::anyhow!(
            "Failed to connect to broker after 30 attempts"
        ))
    }

    async fn health_report_task(
        client: Option<AsyncClient>,
        name: String,
        interval: u64,
        health_report_payload: String,
    ) -> Result<()> {
        info!("🏥 Health report task started for {}", name);
        let mut interval = tokio::time::interval(Duration::from_secs(interval));
        loop {
            interval.tick().await;
            if let Some(client) = &client {
                info!("📤 Sending health report for {}", name);
                match client
                    .publish(
                        &format!("luffy/{}/health", name),
                        QoS::AtLeastOnce,
                        false,
                        health_report_payload.clone(),
                    )
                    .await
                {
                    Ok(_) => debug!("Health report sent successfully"),
                    Err(e) => error!("Failed to send health report: {:?}", e),
                }
            }
        }
    }

    pub fn set_on_message(&mut self, on_message: fn(topic: String, payload: String)) {
        self.on_message = Some(on_message);
    }
}
