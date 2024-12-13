use askama::Template;
use axum::{
    response::{Html, IntoResponse},
    routing::get,
    Json, Router,
};
use serde::Serialize;
use std::env;
use std::sync::Arc;

use crate::{
    config::CONFIG,
    monitor::{mqtt::MqttMonitor, service::ServiceStatus, vehicle::VehicleState},
};
use luffy_common::util;

// View Models
#[derive(Debug, Serialize)]
pub struct StatusViewModel {
    // System info
    pub version: String,
    pub vehicle_id: String,

    // Vehicle state
    pub location: String,
    pub yaw: f32,
    pub battery: f32,
    pub armed: bool,
    pub flight_mode: String,

    // Services
    pub services: Vec<ServiceStatusViewModel>,
}

#[derive(Debug, Serialize)]
pub struct ServiceStatusViewModel {
    pub name: String,
    pub status: String,
    pub last_health_report: String,
}

// Template
#[derive(Template)]
#[template(path = "index.html")]
struct IndexPage {
    status: StatusViewModel,
}

impl From<VehicleState> for StatusViewModel {
    fn from(state: VehicleState) -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            vehicle_id: util::get_vehicle_id(&CONFIG.base),
            location: format!("{:.6}, {:.6}", state.location.0, state.location.1),
            yaw: state.yaw_degree,
            battery: state.battery_percentage,
            armed: state.armed,
            flight_mode: state.flight_mode,
            services: Vec::new(),
        }
    }
}
// Implementation
impl StatusViewModel {
    async fn new() -> Self {
        let (state, services_view) =
            tokio::join!(Self::get_vehicle_state(), Self::get_services_state());

        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            vehicle_id: util::get_vehicle_id(&CONFIG.base),
            location: format!("{:.6}, {:.6}", state.location.0, state.location.1),
            yaw: state.yaw_degree,
            battery: state.battery_percentage,
            armed: state.armed,
            flight_mode: state.flight_mode,
            services: services_view,
        }
    }

    async fn get_vehicle_state() -> VehicleState {
        let monitor = MqttMonitor::instance().await.clone();
        monitor.get_vehicle_snapshot().await.unwrap_or_default()
    }

    async fn get_services_state() -> Vec<ServiceStatusViewModel> {
        let monitor = MqttMonitor::instance().await.clone();
        let services = monitor.get_services_snapshot().await.unwrap_or_default();

        services
            .services
            .iter()
            .map(|(name, state)| ServiceStatusViewModel {
                name: name.clone(),
                status: match state.status {
                    ServiceStatus::Running => "Running".to_string(),
                    ServiceStatus::Stopped => "Stopped".to_string(),
                    ServiceStatus::Unknown => "Unknown".to_string(),
                },
                last_health_report: state
                    .last_health_report
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
                    .to_string(),
            })
            .collect()
    }
}

// Routes and Handlers
pub async fn routes() -> Router {
    Router::new()
        .route("/", get(index_page))
        .route("/api/status", get(status_api))
}

async fn index_page() -> impl IntoResponse {
    let template = IndexPage {
        status: StatusViewModel::new().await,
    };
    Html(template.render().unwrap())
}

async fn status_api() -> impl IntoResponse {
    let monitor = MqttMonitor::instance().await.clone();
    let (vehicle_state, services_view) = tokio::join!(
        monitor.get_vehicle_snapshot(),
        StatusViewModel::get_services_state()
    );

    let mut status = StatusViewModel::from(vehicle_state.unwrap_or_default());
    status.services = services_view;

    Json(status)
}
