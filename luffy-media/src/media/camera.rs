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
use tokio::sync::broadcast;
use tokio::sync::Mutex;
use tracing::{debug, error};
use url::Url;
use webrtc::api::APIBuilder;
use webrtc::ice_transport::ice_connection_state::RTCIceConnectionState;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use webrtc::{
    api::media_engine::MediaEngine,
    ice_transport::ice_server::RTCIceServer,
    media::Sample,
    peer_connection::{
        configuration::RTCConfiguration, sdp::session_description::RTCSessionDescription,
    },
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
};

#[derive(Clone)]
pub struct Camera {
    config: CameraConfig,
    pub running: Arc<AtomicBool>,
    pub frame_sender: broadcast::Sender<Vec<u8>>,
    pub peer_connections: Arc<Mutex<HashMap<String, Arc<RTCPeerConnection>>>>,
}

impl fmt::Debug for Camera {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Camera")
            .field("config", &self.config)
            .field("running", &self.running)
            .field("peer_connections", &self.peer_connections)
            .finish()
    }
}

impl Camera {
    pub fn id(&self) -> &str {
        &self.config.id
    }

    pub async fn new(config: CameraConfig) -> Result<Self> {
        let (frame_sender, _) = broadcast::channel(30); // Buffer size of 30 frames
        Ok(Self {
            config,
            running: Arc::new(AtomicBool::new(false)),
            frame_sender,
            peer_connections: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn start(&self) -> Result<()> {
        if self.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        let mut session = Session::describe(
            Url::parse(&self.config.url)?,
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

        let mut frames = session.demuxed()?;

        let sender = self.frame_sender.clone();
        let running = self.running.clone();

        // Single task reading from RTSP and broadcasting to all peers
        tokio::spawn(async move {
            while running.load(Ordering::SeqCst) {
                match frames.next().await {
                    Some(Ok(CodecItem::VideoFrame(frame))) => {
                        if let Ok(data) = Self::convert_h264(frame) {
                            let _ = sender.send(data); // Send raw frame data
                        }
                    }
                    Some(Err(e)) => {
                        error!("Error reading RTSP frame: {}", e);
                        continue;
                    }
                    _ => continue,
                }
            }
        });

        self.running.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Converts from AVC representation to the Annex B representation expected by webrtc-rs.
    fn convert_h264(frame: VideoFrame) -> Result<Vec<u8>> {
        let mut data = frame.into_data();
        let mut i = 0;
        while i < data.len() - 3 {
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
        let mut peers = self.peer_connections.lock().await;
        for peer in peers.values() {
            if let Err(e) = peer.close().await {
                error!("Error closing peer connection: {}", e);
            }
        }
        peers.clear();
        Ok(())
    }

    pub async fn add_peer(&self, peer_id: &str) -> Result<()> {
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

        self.peer_connections
            .lock()
            .await
            .insert(peer_id.to_string(), peer_connection);

        Ok(())
    }

    pub async fn remove_peer(&self, peer_id: &str) -> Result<()> {
        if let Some(peer) = self.peer_connections.lock().await.remove(peer_id) {
            if let Err(e) = peer.close().await {
                error!("Error closing peer connection: {}", e);
            }
            debug!("Removed peer {}", peer_id);
        }
        Ok(())
    }

    pub async fn handle_offer(&self, request_id: String, offer: String) -> Result<()> {
        if !self.running.load(Ordering::SeqCst) {
            bail!("Camera is not running");
        }

        self.add_peer(&request_id).await?;

        let peer_connection = self
            .peer_connections
            .lock()
            .await
            .get(&request_id)
            .ok_or_else(|| anyhow::anyhow!("Peer connection not found"))?
            .clone();

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

        peer_connection.add_track(track.clone()).await?;

        let mut receiver = self.frame_sender.subscribe();
        let running = self.running.clone();

        // Each peer gets its own receiver
        tokio::spawn(async move {
            while running.load(Ordering::SeqCst) {
                match receiver.recv().await {
                    Ok(data) => {
                        // Directly receive frame data
                        if let Err(e) = track
                            .write_sample(&Sample {
                                data: data.into(),
                                duration: std::time::Duration::from_secs(1),
                                ..Default::default()
                            })
                            .await
                        {
                            error!("Failed to send RTP packet: {}", e);
                        }
                    }
                    Err(e) => error!("Broadcast receive error: {}", e),
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

        // Handle peer connection state changes
        let request_id_state = request_id.clone();
        let camera_self = self.clone();
        peer_connection.on_peer_connection_state_change(Box::new(
            move |s: RTCPeerConnectionState| {
                let request_id = request_id_state.clone();
                let camera = camera_self.clone();
                Box::pin(async move {
                    match s {
                        RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed => {
                            if let Err(e) = camera.remove_peer(&request_id).await {
                                error!("Error removing peer {}: {}", request_id, e);
                            }
                        }
                        _ => (),
                    }
                })
            },
        ));

        // Handle ICE connection state changes
        let request_id_ice = request_id.clone();
        let camera_self = self.clone();
        peer_connection.on_ice_connection_state_change(Box::new(
            move |s: RTCIceConnectionState| {
                let request_id = request_id_ice.clone();
                let camera = camera_self.clone();
                Box::pin(async move {
                    match s {
                        RTCIceConnectionState::Disconnected | RTCIceConnectionState::Failed => {
                            if let Err(e) = camera.remove_peer(&request_id).await {
                                error!("Error removing peer {}: {}", request_id, e);
                            }
                        }
                        _ => (),
                    }
                })
            },
        ));

        Ok(())
    }

    pub async fn add_ice_candidate(
        &self,
        request_id: String,
        candidate: String,
        sdp_mline_index: u32,
    ) -> Result<()> {
        let peer = self
            .peer_connections
            .lock()
            .await
            .get(&request_id)
            .ok_or_else(|| anyhow::anyhow!("Peer connection not found"))?
            .clone();

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
