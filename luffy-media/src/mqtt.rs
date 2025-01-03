use anyhow::Result;
use luffy_common::iot::local::LocalIotClient;
use luffy_common::iot::remote::RemoteIotClient;
use serde_json::json;
use std::sync::Arc;
use std::sync::LazyLock;
use tokio::sync::Mutex;

use tracing::{error, info};

use crate::config::CONFIG;
use crate::media::service::MEDIA_SERVICE;

pub static MQTT_HANDLER: LazyLock<MqttHandler> = LazyLock::new(MqttHandler::new);

pub struct MqttHandler {
    remote_client: Arc<Mutex<RemoteIotClient>>,
    vehicle_id: String,
    local_client: Arc<Mutex<LocalIotClient>>,
}

impl Default for MqttHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl MqttHandler {
    pub fn new() -> Self {
        let vehicle_id = CONFIG.base.vehicle_id.clone();
        let remote_client = Arc::new(Mutex::new(RemoteIotClient::new(
            MqttHandler::on_message,
            vehicle_id.clone(),
            CONFIG.base.aws.iot.endpoint.clone(),
            CONFIG.base.aws.iot.port,
        )));

        let local_client = Arc::new(Mutex::new(LocalIotClient::new(
            "media".to_string(),
            CONFIG.base.mqtt_host.clone(),
            CONFIG.base.mqtt_port,
            None,
            CONFIG.base.health_report_interval,
            env!("CARGO_PKG_VERSION").to_string(),
        )));

        MqttHandler {
            remote_client,
            vehicle_id,
            local_client,
        }
    }

    pub async fn start(&self) -> Result<()> {
        info!("Starting MQTT handler...");
        let mut remote = self.remote_client.lock().await;
        remote.start().await?;
        remote
            .subscribe(&format!("{}/webrtc/request/#", self.vehicle_id))
            .await?;
        let mut local = self.local_client.lock().await;
        local.connect().await?;
        Ok(())
    }

    fn on_message(topic: String, payload: String) {
        tokio::spawn(async move {
            Self::handle_message(topic, payload).await;
        });
    }

    async fn handle_message(topic: String, payload: String) {
        if topic.contains("/webrtc/request/") {
            if let Err(e) = MEDIA_SERVICE.handle_webrtc_message("mqtt", &payload).await {
                error!("Failed to handle WebRTC request: {}", e);
            }
        }
    }

    pub async fn send_webrtc_response(
        &self,
        request_id: &str,
        response: &serde_json::Value,
    ) -> Result<()> {
        let topic = format!("{}/webrtc/response/{}", self.vehicle_id, request_id);
        let payload = response.to_string();
        self.remote_client
            .lock()
            .await
            .publish(&topic, &payload)
            .await?;
        Ok(())
    }

    pub async fn send_webrtc_request(
        &self,
        request_id: &str,
        camera_id: &str,
        offer: &str,
    ) -> Result<()> {
        let payload = json!({
            "type": "offer",
            "request_id": request_id,
            "camera_id": camera_id,
            "offer": offer,
        });
        let topic = format!("{}/webrtc/request/{}", self.vehicle_id, request_id);
        self.remote_client
            .lock()
            .await
            .publish(&topic, &payload.to_string())
            .await?;
        Ok(())
    }

    pub async fn send_ice_candidate(
        &self,
        request_id: &str,
        camera_id: &str,
        candidate: &str,
        sdp_mline_index: u32,
    ) -> Result<()> {
        let payload = json!({
            "type": "candidate",
            "request_id": request_id,
            "camera_id": camera_id,
            "candidate": candidate,
            "sdp_mline_index": sdp_mline_index,
        });
        let topic = format!("{}/webrtc/request/{}", self.vehicle_id, request_id);
        self.remote_client
            .lock()
            .await
            .publish(&topic, &payload.to_string())
            .await?;
        Ok(())
    }
}

pub async fn init_mqtt() -> Result<()> {
    if let Err(e) = MQTT_HANDLER.start().await {
        error!("Failed to start MQTT handler: {}", e);
        return Err(e.into());
    }
    Ok(())
}
