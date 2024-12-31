use anyhow::{bail, Result};
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    routing::get,
    Router,
};
use futures::{stream::SplitSink, SinkExt, StreamExt};
use serde::Deserialize;

use std::sync::Arc;
use std::{collections::HashMap, sync::LazyLock};
use tokio::sync::Mutex;
use tracing::{debug, error, info};
use uuid::Uuid;

use crate::config::CONFIG;
use crate::media::service::WebRTCRequest;
use crate::media::service::MEDIA_SERVICE;

pub static WS_SERVER: LazyLock<WebSocketServer> = LazyLock::new(|| WebSocketServer {
    connections: Arc::new(Mutex::new(HashMap::new())),
});

type WebSocketSink = Arc<Mutex<SplitSink<WebSocket, Message>>>;

pub struct WebSocketServer {
    connections: Arc<Mutex<HashMap<String, WebSocketSink>>>,
}

impl WebSocketServer {
    pub async fn start(&self) -> Result<()> {
        info!("Starting WebSocket server...");

        let app = Router::new().route(
            "/ws",
            get(move |ws: WebSocketUpgrade| async move {
                ws.on_upgrade(move |socket| async move { WS_SERVER.handle_socket(socket).await })
            }),
        );
        let addr = format!("0.0.0.0:{}", CONFIG.websocket_port);
        let addr_str = addr.clone();

        tokio::spawn(async move {
            let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap();
        });

        info!("WebSocket server listening on {}", addr_str);
        Ok(())
    }

    async fn handle_socket(&self, socket: WebSocket) {
        let (ws_sink, mut ws_stream) = socket.split();
        let connection_id = Uuid::new_v4().to_string();
        debug!("New WebSocket connection established: {}", connection_id);

        // Store connection
        self.connections
            .lock()
            .await
            .insert(connection_id.clone(), Arc::new(Mutex::new(ws_sink)));

        // Send connection ID to client
        if let Some(socket) = self.connections.lock().await.get(&connection_id) {
            let init_message = serde_json::json!({
                "type": "connection_id",
                "connection_id": connection_id
            });
            if let Err(e) = socket
                .lock()
                .await
                .send(Message::Text(init_message.to_string()))
                .await
            {
                error!("Failed to send connection ID: {}", e);
                return;
            }
        }

        // Handle incoming messages
        while let Some(result) = ws_stream.next().await {
            match result {
                Ok(msg) => {
                    debug!("Received message from {}", connection_id);
                    match msg {
                        Message::Text(text) => {
                            if let Err(e) = self.handle_message(&connection_id, &text).await {
                                error!("Failed to handle message from {}: {}", connection_id, e);
                            }
                        }
                        Message::Close(reason) => {
                            debug!("Client {} requested close: {:?}", connection_id, reason);
                            break;
                        }
                        _ => debug!(
                            "Ignoring non-text message from {}: {:?}",
                            connection_id, msg
                        ),
                    }
                }
                Err(e) => {
                    error!("WebSocket error for {}: {}", connection_id, e);
                    break;
                }
            }
        }

        // Clean up connection
        debug!("Closing WebSocket connection: {}", connection_id);
        self.connections.lock().await.remove(&connection_id);
    }

    pub async fn send_message(&self, request_id: &str, message: &str) -> Result<()> {
        if let Some(socket) = self.connections.lock().await.get(request_id) {
            debug!("Sending message to connection {}", request_id);
            socket
                .lock()
                .await
                .send(Message::Text(message.to_string()))
                .await
                .map_err(|e| anyhow::anyhow!("Failed to send message: {}", e))
        } else {
            error!("Connection {} not found", request_id);
            Err(anyhow::anyhow!("Connection not found"))
        }
    }

    pub async fn handle_message(&self, connection_id: &str, message: &str) -> Result<()> {
        debug!("Received message from connection {}", connection_id);

        // Add debug logging to see the actual message
        debug!("Message content: {}", message);

        let msg: WebRtcMessage = match serde_json::from_str(message) {
            Ok(msg) => msg,
            Err(e) => {
                error!("Failed to parse message: {}", e);
                debug!("Failed message content: {}", message);
                return Ok(());  // Ignore parsing errors
            }
        };

        match msg {
            WebRtcMessage::WebRTC {
                message_type,
                camera_id,
                data,
            } => {
                let request = match (message_type.as_str(), data) {
                    ("offer", WebRTCData::Offer { offer }) => WebRTCRequest::Offer {
                        camera_id,
                        request_id: connection_id.to_string(),
                        offer,
                    },
                    ("candidate", WebRTCData::Candidate { 
                        candidate, 
                        sdp_mline_index,
                        ..  // Ignore sdp_mid as it's optional
                    }) => WebRTCRequest::Candidate {
                        camera_id,
                        request_id: connection_id.to_string(),
                        candidate,
                        sdp_mline_index,
                    },
                    _ => {
                        debug!("Ignoring WebRTC message with type: {}", message_type);
                        return Ok(());
                    }
                };

                debug!("Processing WebRTC {}", message_type);
                MEDIA_SERVICE.handle_webrtc_request(request).await?;
            }
            WebRtcMessage::ConnectionResponse { .. } => {
                debug!("Ignoring connection response message");
            }
            WebRtcMessage::Other(_) => {
                debug!("Ignoring non-WebRTC message");
            }
        }

        Ok(())
    }
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum WebRtcMessage {
    WebRTC {
        #[serde(rename = "type")]
        message_type: String,
        camera_id: String,
        #[serde(flatten)]
        data: WebRTCData,
    },
    ConnectionResponse {
        #[serde(rename = "type")]
        message_type: String,
        connection_id: String,
    },
    Other(serde_json::Value),
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum WebRTCData {
    Offer {
        offer: String,
    },
    Candidate {
        candidate: String,
        sdp_mline_index: u32,
        sdp_mid: Option<String>,
    },
}
