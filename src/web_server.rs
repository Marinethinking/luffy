use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use axum::{http::StatusCode, routing::get, Json, Router};

use serde_json::Value;
use tokio::sync::OnceCell;

use crate::vehicle::{Vehicle, VehicleState};

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
        let app = Router::new().route(
            "/api/vehicle/state",
            get(move || async move {
                Json(
                    serde_json::to_value(vehicle.get_state_snapshot().unwrap_or_default()).unwrap(),
                )
            }),
        );

        self.running.store(true, Ordering::SeqCst);

        let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
            .await
            .context("Failed to bind to port 3000")?;
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
