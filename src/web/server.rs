use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use axum::{routing::get, Router};
use tower_http::services::ServeDir;

use super::index_page;
use crate::{config::CONFIG, vehicle::Vehicle};

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

        // Create the main router
        let app = Router::new()
            // Merge routes from index_page
            .merge(index_page::routes(vehicle))
            // Serve static files
            .nest_service("/static", ServeDir::new("static"));

        self.running.store(true, Ordering::SeqCst);

        let host = CONFIG.web.host.clone();
        let port = CONFIG.web.port;
        println!("Starting web server on http://{}:{}", host, port);

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
