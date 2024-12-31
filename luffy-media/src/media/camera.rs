use crate::config::CameraConfig;
use crate::ws::WS_SERVER;
use anyhow::{bail, Result};
use futures::StreamExt;
use retina::client::{Demuxed, PlayOptions, Playing};
use retina::client::{Session, SessionOptions, SetupOptions};
use retina::codec::{CodecItem, VideoFrame};
use std::collections::HashMap;
use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::Mutex;
use tracing::{debug, error, info};

use webrtc::api::APIBuilder;
use webrtc::ice_transport::ice_candidate::RTCIceCandidate;
use webrtc::ice_transport::ice_connection_state::RTCIceConnectionState;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp_transceiver::rtp_codec::{RTCRtpCodecParameters, RTPCodecType};
use webrtc::rtp_transceiver::rtp_transceiver_direction::RTCRtpTransceiverDirection;
use webrtc::rtp_transceiver::RTCPFeedback;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use webrtc::track::track_local::TrackLocal;
use webrtc::{
    api::media_engine::MediaEngine,
    ice_transport::ice_server::RTCIceServer,
    media::Sample,
    peer_connection::{
        configuration::RTCConfiguration, sdp::session_description::RTCSessionDescription,
    },
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
};

use super::service::WebRTCRequest;

#[derive(Clone)]
pub struct Camera {
    config: CameraConfig,
    pub running: Arc<AtomicBool>,
    pub peer_connections: Arc<Mutex<HashMap<String, Arc<RTCPeerConnection>>>>,
    pending_candidates: Arc<Mutex<HashMap<String, VecDeque<(String, u32)>>>>,
    video_tracks: Arc<Mutex<Vec<Arc<TrackLocalStaticSample>>>>,
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
        Ok(Self {
            config,
            running: Arc::new(AtomicBool::new(false)),
            peer_connections: Arc::new(Mutex::new(HashMap::new())),
            pending_candidates: Arc::new(Mutex::new(HashMap::new())),
            video_tracks: Arc::new(Mutex::new(Vec::new())),
        })
    }

    pub async fn start(&self) -> Result<()> {
        self.running.store(true, Ordering::SeqCst);

        let camera_id = self.id().to_string();
        let url = self.config.url.clone();
        let running = self.running.clone();
        let video_tracks = self.video_tracks.clone();

        tokio::spawn(async move {
            while running.load(Ordering::SeqCst) {
                match Self::setup_rtsp_stream(&camera_id, &url, video_tracks.clone()).await {
                    Ok(_) => {
                        while running.load(Ordering::SeqCst) {
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        }
                    }
                    Err(e) => {
                        error!("Failed to connect to RTSP stream: {}. Retrying in 5s", e);
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                }
            }
        });

        Ok(())
    }

    /// Converts from AVC representation to the Annex B representation expected by webrtc-rs.
    fn convert_h264(frame: VideoFrame) -> Result<Vec<u8>> {
        // Convert from AVC to Annex B format
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

    async fn setup_rtsp_stream(
        camera_id: &str,
        url: &str,
        video_tracks: Arc<Mutex<Vec<Arc<TrackLocalStaticSample>>>>,
    ) -> Result<()> {
        let mut session = Session::describe(url.parse()?, SessionOptions::default()).await?;
        let video = session
            .streams()
            .iter()
            .position(|s| s.media() == "video")
            .ok_or_else(|| anyhow::anyhow!("No video track found"))?;

        // Get H264 parameters from the stream
        let stream = &session.streams()[video];
        let extra_data = stream
            .parameters()
            .ok_or_else(|| anyhow::anyhow!("No H264 parameters found"))?;

        session.setup(video, SetupOptions::default()).await?;
        let session = session.play(PlayOptions::default()).await?;
        let mut frames = session.demuxed()?;

        // Send H264 parameters first
        let tracks = video_tracks.lock().await;
        if !tracks.is_empty() {
            let sample = Sample {
                data: vec![
                    0x00, 0x00, 0x00, 0x01, 0x67, 0x42, 0x00, 0x1f, 0x96, 0x54, 0x0b, 0x24, 0x00,
                    0x00, 0x00, 0x01, 0x68, 0xce, 0x38, 0x80,
                ]
                .into(),
                duration: std::time::Duration::from_secs(1) / 30,
                timestamp: std::time::SystemTime::now(),
                packet_timestamp: 0,
                prev_dropped_packets: 0,
                prev_padding_packets: 0,
            };

            for track in tracks.iter() {
                track.write_sample(&sample).await?;
            }
        }
        drop(tracks);

        while let Some(frame) = frames.next().await {
            match frame {
                Ok(CodecItem::VideoFrame(video_frame)) => {
                    let frame_data = Self::convert_h264(video_frame)?;

                    let sample = Sample {
                        data: frame_data.into(),
                        duration: std::time::Duration::from_secs(1) / 30,
                        timestamp: std::time::SystemTime::now(),
                        packet_timestamp: 0,
                        prev_dropped_packets: 0,
                        prev_padding_packets: 0,
                    };

                    // Hold lock while writing to all tracks
                    let tracks = video_tracks.lock().await;
                    if !tracks.is_empty() {
                        for track in tracks.iter() {
                            if let Err(e) = track.write_sample(&sample).await {
                                error!("Failed to write sample: {}", e);
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Error receiving frame: {}", e);
                    break;
                }
                _ => continue,
            }
        }
        Ok(())
    }

    pub async fn handle_offer(&self, request_id: String, offer: String) -> Result<()> {
        info!("Handling offer for camera {}", self.id());
        if !self.running.load(Ordering::SeqCst) {
            bail!("Camera is not running");
        }

        debug!("Creating peer connection for {}", request_id);
        let peer_connection = {
            let mut media_engine = MediaEngine::default();
            media_engine.register_default_codecs()?;

            let api = APIBuilder::new().with_media_engine(media_engine).build();

            let config = RTCConfiguration {
                ice_servers: vec![
                    RTCIceServer {
                        urls: vec!["stun:stun1.l.google.com:19302".to_owned()],
                        ..Default::default()
                    },
                    RTCIceServer {
                        urls: vec!["stun:stun2.l.google.com:19302".to_owned()],
                        ..Default::default()
                    },
                ],
                ..Default::default()
            };

            Arc::new(api.new_peer_connection(config).await?)
        };

        // Store peer connection before setting descriptions
        self.peer_connections
            .lock()
            .await
            .insert(request_id.clone(), peer_connection.clone());

        // Create video track
        let video_track = Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability {
                mime_type: "video/H264".to_owned(),
                clock_rate: 90000,
                channels: 0,
                sdp_fmtp_line:
                    "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42001f"
                        .to_owned(),
                rtcp_feedback: vec![
                    RTCPFeedback {
                        typ: "nack".to_owned(),
                        parameter: "".to_owned(),
                    },
                    RTCPFeedback {
                        typ: "nack".to_owned(),
                        parameter: "pli".to_owned(),
                    },
                    RTCPFeedback {
                        typ: "ccm".to_owned(),
                        parameter: "fir".to_owned(),
                    },
                ],
            },
            format!("video-{}", self.id()),
            format!("webrtc-rs"),
        ));

        // Add track to peer connection first
        peer_connection.add_track(video_track.clone()).await?;

        // Add track to broadcast list
        self.video_tracks.lock().await.push(video_track.clone());

        // Handle ICE connection state changes
        let request_id_clone = request_id.clone();
        peer_connection.on_ice_connection_state_change(Box::new(
            move |state: RTCIceConnectionState| {
                debug!(
                    "ICE connection state changed for {}: {:?}",
                    request_id_clone, state
                );
                Box::pin(async {})
            },
        ));

        // Handle connection state changes
        let request_id_clone = request_id.clone();
        peer_connection.on_peer_connection_state_change(Box::new(
            move |state: RTCPeerConnectionState| {
                debug!(
                    "Peer connection state changed for {}: {:?}",
                    request_id_clone, state
                );
                Box::pin(async {})
            },
        ));

        debug!("Setting remote description for peer {}", request_id);
        let offer = RTCSessionDescription::offer(offer)?;
        peer_connection.set_remote_description(offer).await?;

        debug!("Creating answer for peer {}", request_id);
        let answer = peer_connection.create_answer(None).await?;

        debug!("Setting local description for peer {}", request_id);
        peer_connection
            .set_local_description(answer.clone())
            .await?;

        // Process any pending candidates
        let mut pending = self.pending_candidates.lock().await;
        if let Some(candidates) = pending.remove(&request_id) {
            for (candidate, sdp_mline_index) in candidates {
                debug!("Processing pending ICE candidate for peer {}", request_id);
                self.add_ice_candidate_internal(&peer_connection, candidate, sdp_mline_index)
                    .await?;
            }
        }

        // Send answer using the WebSocket connection ID
        let response = serde_json::json!({
            "type": "answer",
            "request_id": request_id,
            "camera_id": self.id(),
            "answer": answer.sdp,
        });

        // Send the response through WS_SERVER
        debug!("Sending answer for request {}", request_id);
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
        debug!(
            "Adding ICE candidate for peer {}: {} (mline: {})",
            request_id, candidate, sdp_mline_index
        );

        let peer = {
            let peers = self.peer_connections.lock().await;
            match peers.get(&request_id) {
                Some(p) => p.clone(),
                None => {
                    // Queue the candidate if peer isn't ready
                    let mut pending = self.pending_candidates.lock().await;
                    pending
                        .entry(request_id.clone())
                        .or_insert_with(VecDeque::new)
                        .push_back((candidate, sdp_mline_index));
                    debug!("Queued ICE candidate for peer {}", request_id);
                    return Ok(());
                }
            }
        };

        if peer.remote_description().await.is_none() {
            // Queue the candidate if remote description isn't set
            let mut pending = self.pending_candidates.lock().await;
            pending
                .entry(request_id.clone())
                .or_insert_with(VecDeque::new)
                .push_back((candidate, sdp_mline_index));
            debug!(
                "Queued ICE candidate for peer {} (waiting for remote description)",
                request_id
            );
            return Ok(());
        }

        self.add_ice_candidate_internal(&peer, candidate, sdp_mline_index)
            .await
    }

    async fn add_ice_candidate_internal(
        &self,
        peer: &RTCPeerConnection,
        candidate: String,
        sdp_mline_index: u32,
    ) -> Result<()> {
        let candidate_init = webrtc::ice_transport::ice_candidate::RTCIceCandidateInit {
            candidate,
            sdp_mid: None,
            sdp_mline_index: Some(sdp_mline_index as u16),
            username_fragment: None,
        };

        peer.add_ice_candidate(candidate_init).await?;
        debug!("Successfully added ICE candidate");
        Ok(())
    }
}
