use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use axum::Router;
use tower_http::services::ServeDir;

use super::index_page;
use crate::config::CONFIG;

use anyhow::{Context, Result};

pub struct WebServer {
    running: Arc<AtomicBool>,
}

impl WebServer {
    pub async fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(true)),
        }
    }

    pub async fn start(&self) -> Result<()> {
        // Get static directory path
        let static_dir = if cfg!(debug_assertions) {
            std::env::current_dir()?
                .join("luffy-launcher")
                .join("static")
        } else {
            std::env::current_exe()?
                .parent()
                .context("Failed to get executable directory")?
                .join("static")
        };

        let app = Router::new()
            .merge(index_page::routes().await)
            .nest_service("/static", ServeDir::new(&static_dir));

        let host = CONFIG.web.host.clone();
        let port = CONFIG.web.port;

        let listener = tokio::net::TcpListener::bind(format!("{}:{}", host, port)).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }

    async fn shutdown_signal(&self) {
        while self.running.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    pub async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}
