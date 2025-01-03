use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use std::sync::{Arc, LazyLock};
use tokio::sync::Mutex;
use tracing::{debug, error, info};

use crate::config::CONFIG;
use crate::media::camera::Camera;

pub static MEDIA_SERVICE: LazyLock<Arc<MediaService>> = LazyLock::new(|| {
    Arc::new(MediaService {
        cameras: Arc::new(Mutex::new(HashMap::new())),
    })
});

#[derive(Debug, Deserialize)]
pub struct WebRTCMessage {
    #[serde(rename = "type")]
    pub message_type: String,
    pub camera_id: String,
    pub offer: Option<String>,
    pub candidate: Option<String>,
    pub sdp_mline_index: Option<u32>,
    pub sdp_mid: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WebRTCResponse {
    Answer {
        camera_id: String,
        peer_id: String,
        answer: String,
    },
    Candidate {
        camera_id: String,
        peer_id: String,
        candidate: String,
        sdp_mline_index: u32,
    },
}

#[derive(Debug)]
pub struct MediaService {
    cameras: Arc<Mutex<HashMap<String, Arc<Camera>>>>,
}

impl MediaService {
    pub async fn new() -> Result<Arc<Self>> {
        let service = Arc::new(Self {
            cameras: Arc::new(Mutex::new(HashMap::new())),
        });

        Ok(service)
    }

    pub async fn start(&self) -> Result<()> {
        info!("Starting media service...");

        // Initialize cameras
        info!("Initializing cameras from config: {:?}", CONFIG.cameras);
        for camera_config in &CONFIG.cameras {
            self.add_camera(camera_config.clone()).await?;
        }

        let cameras = self.list_cameras().await;
        info!("Initialized cameras: {:?}", cameras);
        Ok(())
    }

    pub async fn stop(&self) {
        info!("Stopping media service...");
        let cameras = self.cameras.lock().await;
        for camera in cameras.values() {
            if let Err(e) = camera.stop().await {
                error!("Failed to stop camera {}: {}", camera.id(), e);
            }
        }
    }

    // Camera management
    pub async fn add_camera(&self, camera_config: crate::config::CameraConfig) -> Result<()> {
        info!(
            "Adding camera {} with pipeline {}",
            camera_config.id.clone(),
            camera_config.url
        );
        let mut cameras = self.cameras.lock().await;
        let camera = Arc::new(Camera::new(camera_config.clone()).await?);
        camera.start().await?;
        cameras.insert(camera_config.id.clone(), camera);
        Ok(())
    }

    pub async fn remove_camera(&self, camera_id: &str) -> Result<()> {
        let mut cameras = self.cameras.lock().await;
        if let Some(camera) = cameras.remove(camera_id) {
            camera.stop().await?;
        }
        Ok(())
    }

    pub async fn get_camera(&self, id: &str) -> Option<Arc<Camera>> {
        let cameras = self.cameras.lock().await;
        cameras.get(id).cloned()
    }

    pub async fn list_cameras(&self) -> Vec<String> {
        let cameras = self.cameras.lock().await;
        cameras.keys().cloned().collect()
    }

    // WebRTC handling
    pub async fn handle_webrtc_message(&self, connection_id: &str, message: &str) -> Result<()> {
        info!("Handling WebRTC message, connection_id: {}", connection_id);
        let msg: WebRTCMessage = serde_json::from_str(message).map_err(|e| {
            error!("Failed to parse message: {}", e);
            anyhow::anyhow!("Invalid message format")
        })?;

        let connection_id = connection_id.to_string();
        let action = match msg.message_type.as_str() {
            "offer" => {
                let offer = msg.offer.ok_or_else(|| anyhow::anyhow!("Missing offer"))?;
                Box::pin(async move {
                    let camera = MEDIA_SERVICE
                        .get_camera(&msg.camera_id)
                        .await
                        .ok_or_else(|| anyhow::anyhow!("Camera not found"))?;
                    camera.handle_offer(connection_id, offer).await
                }) as Pin<Box<dyn Future<Output = Result<()>> + Send>>
            }
            "candidate" => {
                let candidate = msg
                    .candidate
                    .ok_or_else(|| anyhow::anyhow!("Missing candidate"))?;
                let sdp_mline_index = msg
                    .sdp_mline_index
                    .ok_or_else(|| anyhow::anyhow!("Missing sdp_mline_index"))?;
                Box::pin(async move {
                    let camera = MEDIA_SERVICE
                        .get_camera(&msg.camera_id)
                        .await
                        .ok_or_else(|| anyhow::anyhow!("Camera not found"))?;
                    camera
                        .add_ice_candidate(connection_id, candidate, sdp_mline_index)
                        .await
                }) as Pin<Box<dyn Future<Output = Result<()>> + Send>>
            }
            _ => return Ok(()),
        };

        debug!("Processing WebRTC {}", msg.message_type);
        action.await
    }
}
