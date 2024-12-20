use anyhow::Result;
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    routing::get,
    Router,
};
use futures::{stream::SplitSink, SinkExt, StreamExt};

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
        let request_id = Uuid::new_v4().to_string();
        debug!("New WebSocket connection established: {}", request_id);

        // Store connection
        self.connections
            .lock()
            .await
            .insert(request_id.clone(), Arc::new(Mutex::new(ws_sink)));

        // Handle incoming messages
        while let Some(result) = ws_stream.next().await {
            match result {
                Ok(msg) => {
                    debug!("Received message from {}", request_id);
                    match msg {
                        Message::Text(text) => match serde_json::from_str::<WebRTCRequest>(&text) {
                            Ok(request) => {
                                if let Err(e) = self.handle_webrtc_request(request).await {
                                    error!(
                                        "Failed to handle WebRTC request from {}: {}",
                                        request_id, e
                                    );
                                }
                            }
                            Err(e) => {
                                error!("Failed to parse WebRTC request from {}: {}", request_id, e);
                            }
                        },
                        Message::Close(reason) => {
                            debug!("Client {} requested close: {:?}", request_id, reason);
                            break;
                        }
                        _ => debug!("Ignoring non-text message from {}: {:?}", request_id, msg),
                    }
                }
                Err(e) => {
                    error!("WebSocket error for {}: {}", request_id, e);
                    break;
                }
            }
        }

        // Clean up connection
        debug!("Closing WebSocket connection: {}", request_id);
        self.connections.lock().await.remove(&request_id);
    }

    pub async fn send_message(&self, request_id: &str, message: &str) -> Result<()> {
        match self.connections.lock().await.get(request_id) {
            Some(socket) => {
                debug!("Sending message to {}: {}", request_id, message);
                socket
                    .lock()
                    .await
                    .send(Message::Text(message.to_string()))
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to send message: {}", e))
            }
            None => {
                error!(
                    "Attempted to send message to non-existent connection: {}",
                    request_id
                );
                Err(anyhow::anyhow!("Connection not found"))
            }
        }
    }

    async fn handle_webrtc_request(&self, request: WebRTCRequest) -> Result<()> {
        debug!("Processing WebRTC request");
        MEDIA_SERVICE
            .handle_webrtc_request(request)
            .await
            .map_err(|e| anyhow::anyhow!("WebRTC request handling failed: {}", e))
    }
}
