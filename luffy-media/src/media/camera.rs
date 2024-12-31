use crate::config::CameraConfig;
use crate::ws::WS_SERVER;
use anyhow::{bail, Result};
use futures::StreamExt;
use retina::client::{Demuxed, PlayOptions, Playing};
use retina::client::{Session, SessionOptions, SetupOptions};
use retina::codec::{CodecItem, VideoFrame};
use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::debug;
use url::Url;
use webrtc::api::media_engine::MediaEngine;
use webrtc::peer_connection::RTCPeerConnection;

use webrtc::{
    api::{interceptor_registry::register_default_interceptors, APIBuilder},
    ice_transport::{ice_connection_state::RTCIceConnectionState, ice_server::RTCIceServer},
    interceptor::registry::Registry,
    media::Sample,
    peer_connection::{
        configuration::RTCConfiguration, peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription,
    },
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::track_local::{track_local_static_sample::TrackLocalStaticSample, TrackLocal},
};

#[derive(Clone)]
pub struct Camera {
    config: CameraConfig,
    pub running: Arc<AtomicBool>,
    pub peer_connections: Arc<Mutex<HashMap<String, Arc<RTCPeerConnection>>>>,
}

impl fmt::Debug for Camera {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Camera")
            .field("config", &self.config)
            .field("running", &self.running)
            .field("peer_connections", &self.peer_connections)
            // Skip rtsp_session as it doesn't implement Debug
            .finish()
    }
}

impl Camera {
    pub fn id(&self) -> &str {
        &self.config.id
    }

    pub async fn new(config: CameraConfig) -> Result<Self> {
        Ok(Self {
            config,
            running: Arc::new(AtomicBool::new(false)),
            peer_connections: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn start(&self) -> Result<()> {
        if self.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        // Just mark as running, actual streaming starts when peers connect
        self.running.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Converts from AVC representation to the Annex B representation expected by webrtc-rs.
    fn convert_h264(frame: VideoFrame) -> Result<Vec<u8>> {
        let mut data = frame.into_data();
        let mut i = 0;
        while i < data.len() - 3 {
            // Replace each NAL's length with the Annex B start code b"\x00\x00\x00\x01".
            let len = u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
            data[i] = 0;
            data[i + 1] = 0;
            data[i + 2] = 0;
            data[i + 3] = 1;
            i += 4 + len;
            if i > data.len() {
                bail!("partial NAL body");
            }
        }
        if i < data.len() {
            bail!("partial NAL length");
        }
        Ok(data)
    }

    pub async fn stop(&self) -> Result<()> {
        self.running.store(false, Ordering::SeqCst);

        // Close all peer connections
        let mut peers = self.peer_connections.lock().await;
        for peer in peers.values() {
            if let Err(e) = peer.close().await {
                tracing::error!("Error closing peer connection: {}", e);
            }
        }
        peers.clear();

        Ok(())
    }

    pub async fn add_peer(&self, peer_id: &str) -> Result<()> {
        // Create WebRTC API with media engine
        let mut media_engine = MediaEngine::default();
        media_engine.register_default_codecs()?;

        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut media_engine)?;

        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .build();

        // Create WebRTC peer connection
        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                ..Default::default()
            }],
            ..Default::default()
        };

        let peer_connection = Arc::new(
            APIBuilder::new()
                .build()
                .new_peer_connection(config)
                .await?,
        );

        // Store peer connection
        self.peer_connections
            .lock()
            .await
            .insert(peer_id.to_string(), Arc::clone(&peer_connection));

        Ok(())
    }

    pub async fn remove_peer(&self, peer_id: &str) -> Result<()> {
        let mut peers = self.peer_connections.lock().await;
        if let Some(peer) = peers.remove(peer_id) {
            if let Err(e) = peer.close().await {
                tracing::error!("Error closing peer connection: {}", e);
            }
            tracing::info!("Removed peer {}", peer_id);
        }
        Ok(())
    }

    pub async fn handle_offer(&self, request_id: String, offer: String) -> Result<()> {
        // Check if we're running
        if !self.running.load(Ordering::SeqCst) {
            return Err(anyhow::anyhow!("Camera is not running"));
        }

        // Add peer first to create the peer connection
        self.add_peer(&request_id).await?;

        // Get the peer connection
        let peer_connection = self
            .peer_connections
            .lock()
            .await
            .get(&request_id)
            .ok_or_else(|| anyhow::anyhow!("Peer connection not found"))?
            .clone();

        // Create RTSP session
        let mut session = Session::describe(
            Url::parse(&self.config.pipeline_str)?,
            SessionOptions::default().user_agent("luffy-media".to_owned()),
        )
        .await?;

        let video = session
            .streams()
            .iter()
            .position(|s| s.media() == "video")
            .ok_or_else(|| anyhow::anyhow!("No video track found"))?;
        session.setup(video, SetupOptions::default()).await?;
        let session = session.play(PlayOptions::default()).await?;

        // Get frame stream
        let mut frames = session.demuxed()?;

        // Create and add track
        let track = Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability {
                mime_type: "video/H264".to_owned(),
                clock_rate: 90000,
                channels: 0,
                sdp_fmtp_line:
                    "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f"
                        .to_owned(),
                ..Default::default()
            },
            "video".to_owned(),
            "luffy-media".to_owned(),
        ));

        let track_cloned = track.clone();
        peer_connection.add_track(track).await?;

        // Start streaming for this peer
        let running = self.running.clone();
        tokio::spawn(async move {
            while running.load(Ordering::SeqCst) {
                match frames.next().await {
                    Some(Ok(CodecItem::VideoFrame(frame))) => {
                        let data = match Self::convert_h264(frame) {
                            Ok(data) => data,
                            Err(e) => {
                                tracing::error!("Failed to convert H264 frame: {}", e);
                                continue;
                            }
                        };
                        if let Err(e) = track_cloned
                            .write_sample(&Sample {
                                data: data.into(),
                                duration: std::time::Duration::from_secs(1),
                                ..Default::default()
                            })
                            .await
                        {
                            tracing::error!("Failed to send RTP packet: {}", e);
                        }
                    }
                    Some(Ok(_)) => (),
                    Some(Err(e)) => tracing::error!("Error reading frame: {}", e),
                    None => break,
                }
            }
        });

        // Handle WebRTC setup
        let offer = RTCSessionDescription::offer(offer)?;
        peer_connection.set_remote_description(offer).await?;
        let answer = peer_connection.create_answer(None).await?;
        peer_connection
            .set_local_description(answer.clone())
            .await?;

        // Store peer connection
        self.peer_connections
            .lock()
            .await
            .insert(request_id.clone(), peer_connection);

        // Send answer
        let response = serde_json::json!({
            "type": "answer",
            "request_id": request_id,
            "camera_id": self.id(),
            "answer": answer.sdp,
        });

        WS_SERVER
            .send_message(&request_id, &response.to_string())
            .await?;
        Ok(())
    }

    pub async fn add_ice_candidate(
        &self,
        request_id: String,
        candidate: String,
        sdp_mline_index: u32,
    ) -> Result<()> {
        let peers = self.peer_connections.lock().await;
        let peer = peers
            .get(&request_id)
            .ok_or_else(|| anyhow::anyhow!("Peer connection not found"))?;

        peer.add_ice_candidate(webrtc::ice_transport::ice_candidate::RTCIceCandidateInit {
            candidate,
            sdp_mid: None,
            sdp_mline_index: Some(sdp_mline_index as u16),
            username_fragment: None,
        })
        .await?;

        Ok(())
    }
}
