use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use tokio::sync::Mutex;
use tracing::{error, info};

use crate::config::CONFIG;
use crate::media::camera::Camera;

pub static MEDIA_SERVICE: LazyLock<Arc<MediaService>> = LazyLock::new(|| {
    Arc::new(MediaService {
        cameras: Arc::new(Mutex::new(HashMap::new())),
    })
});

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum WebRTCRequest {
    Offer {
        camera_id: String,
        request_id: String,
        offer: String,
    },
    Candidate {
        request_id: String,
        camera_id: String,
        candidate: String,
        sdp_mline_index: u32,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WebRTCResponse {
    Answer {
        request_id: String,
        camera_id: String,
        answer: String,
    },
    Candidate {
        request_id: String,
        camera_id: String,
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
    pub async fn handle_webrtc_request(&self, request: WebRTCRequest) -> Result<()> {
        info!("Received WebRTC request");
        match request {
            WebRTCRequest::Offer {
                request_id,
                camera_id,
                offer,
            } => {
                if let Some(camera) = self.get_camera(&camera_id).await {
                    camera.handle_offer(request_id, offer).await?;
                } else {
                    error!("Camera {} not found", camera_id);
                }
            }
            WebRTCRequest::Candidate {
                request_id,
                camera_id,
                candidate,
                sdp_mline_index,
            } => {
                if let Some(camera) = self.get_camera(&camera_id).await {
                    camera
                        .add_ice_candidate(request_id, candidate, sdp_mline_index)
                        .await?;
                } else {
                    error!("Camera {} not found", camera_id);
                }
            }
        }
        Ok(())
    }
}

impl WebRTCRequest {
    pub fn request_id(&self) -> &str {
        match self {
            WebRTCRequest::Offer { request_id, .. } => request_id,
            WebRTCRequest::Candidate { request_id, .. } => request_id,
        }
    }
}
