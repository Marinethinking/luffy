use super::camera::Camera;
use anyhow::Result;
use std::sync::LazyLock;
use tracing_subscriber::{self, fmt::format::FmtSpan};
use webrtc::api::APIBuilder;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::configuration::RTCConfiguration;

static TRACING: LazyLock<()> = LazyLock::new(|| {
    tracing_subscriber::fmt()
        .with_span_events(FmtSpan::CLOSE)
        .with_test_writer()
        .init();
});

// Common test setup function
async fn setup_test() -> Result<Camera> {
    // Ensure tracing is initialized
    LazyLock::force(&TRACING);

    // Create test camera config
    let config = crate::config::CameraConfig {
        id: "test_camera".to_string(),
        name: "Test Camera".to_string(),
        url: "rtsp://127.0.0.1:8554/test".to_string(),
    };

    // Create and return camera instance
    Ok(Camera::new(config).await?)
}

#[tokio::test]
async fn test_camera_rtsp_connection() -> Result<()> {
    let camera = setup_test().await?;
    camera.start().await?;

    // Verify camera is running
    assert!(camera.running.load(std::sync::atomic::Ordering::SeqCst));

    Ok(())
}

#[tokio::test]
async fn test_webrtc_peer_creation() -> Result<()> {
    let camera = setup_test().await?;
    camera.start().await?;

    // Create a test offer
    let api = APIBuilder::new().build();
    let peer_connection = api
        .new_peer_connection(RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                ..Default::default()
            }],
            ..Default::default()
        })
        .await?;

    // Create offer
    let offer = peer_connection.create_offer(None).await?;

    // Test handling the offer
    camera
        .handle_offer("test_peer".to_string(), offer.sdp)
        .await?;

    // Verify peer connection was created
    let peers = camera.peer_connections.lock().await;
    assert!(peers.contains_key("test_peer"));

    Ok(())
}
