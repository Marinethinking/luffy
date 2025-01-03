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

use crate::{config::CONFIG, media::service::MEDIA_SERVICE};

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
        debug!("New WebSocket connection established");

        let ws_sink = Arc::new(Mutex::new(ws_sink));

        // Handle incoming messages
        while let Some(result) = ws_stream.next().await {
            match result {
                Ok(msg) => {
                    match msg {
                        Message::Text(text) => {
                            // Parse message to get request_id
                            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&text) {
                                if let Some(request_id) =
                                    msg.get("request_id").and_then(|v| v.as_str())
                                {
                                    info!("Received request_id: {}", request_id);

                                    // Scope the lock to drop it before handle_message
                                    {
                                        let mut connections = self.connections.lock().await;
                                        if !connections.contains_key(request_id) {
                                            debug!(
                                                "Storing new WebSocket connection for request_id: {}",
                                                request_id
                                            );
                                            connections
                                                .insert(request_id.to_string(), ws_sink.clone());
                                        }
                                    } // Lock is dropped here

                                    if let Err(e) = self.handle_message(request_id, &text).await {
                                        error!("Failed to handle message: {}", e);
                                    }
                                }
                            }
                        }
                        Message::Close(reason) => {
                            debug!("Client requested close: {:?}", reason);
                            break;
                        }
                        _ => debug!("Ignoring non-text message"),
                    }
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    break;
                }
            }
        }

        debug!("WebSocket connection closed");
    }

    pub async fn send_message(&self, request_id: &str, message: &str) -> Result<()> {
        info!("Sending message to connection {}", request_id);
        if let Some(socket) = self.connections.lock().await.get(request_id) {
            debug!("Sending message to connection {}", request_id);
            return socket
                .lock()
                .await
                .send(Message::Text(message.to_string()))
                .await
                .map_err(|e| anyhow::anyhow!("Failed to send message: {}", e));
        }
        error!("Connection mapping not found for request {}", request_id);
        Err(anyhow::anyhow!("Connection not found"))
    }

    pub async fn handle_message(&self, request_id: &str, message: &str) -> Result<()> {
        MEDIA_SERVICE
            .handle_webrtc_message(request_id, message)
            .await
    }
}
