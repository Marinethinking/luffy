use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use axum::Router;
use tower_http::services::ServeDir;

use super::index_page;
use crate::{
    config::{LauncherConfig, CONFIG},
    monitor::vehicle::Vehicle,
};

use anyhow::{Context, Result};

pub struct WebServer {
    vehicle: &'static Vehicle,
    running: Arc<AtomicBool>,
}

impl WebServer {
    pub async fn new() -> Self {
        Self {
            vehicle: Vehicle::instance().await,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn start(&self) -> Result<()> {
        let vehicle = self.vehicle;

        // For development, first try the luffy-launcher/static directory
        let static_dir = if cfg!(debug_assertions) {
            // Get workspace root
            let workspace_root = std::env::current_dir()?;

            // Try luffy-launcher/static first
            let launcher_static = workspace_root.join("luffy-launcher").join("static");
            if launcher_static.exists() {
                launcher_static
            } else {
                // Fallback to executable directory
                std::env::current_exe()?
                    .parent()
                    .context("Failed to get executable directory")?
                    .join("static")
            }
        } else {
            // Production: use executable adjacent path
            std::env::current_exe()?
                .parent()
                .context("Failed to get executable directory")?
                .join("static")
        };

        // Add debug logging
        tracing::debug!("Current working directory: {:?}", std::env::current_dir()?);
        tracing::debug!("Static directory path: {:?}", static_dir);

        // Verify the static directory exists
        if !static_dir.exists() {
            tracing::warn!("Static directory does not exist at {:?}", static_dir);
            tracing::warn!(
                "Please create the static directory at: {}",
                std::env::current_dir()?
                    .join("luffy-launcher")
                    .join("static")
                    .display()
            );
        }

        // Create the main router
        let app = Router::new()
            .merge(index_page::routes(vehicle))
            .nest_service("/static", ServeDir::new(&static_dir));

        self.running.store(true, Ordering::SeqCst);

        let host = CONFIG.web.host.clone();
        let port = CONFIG.web.port;
        tracing::info!("Starting web server on http://{}:{}", host, port);

        let listener = tokio::net::TcpListener::bind(format!("{}:{}", host, port))
            .await
            .context(format!("Failed to bind to port {}", port))?;

        axum::serve(listener, app)
            .await
            .context("Failed to serve")?;

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
