use std::env;

use crate::config::CONFIG;
use crate::monitor::vehicle::{Vehicle, VehicleState};
use askama::Template;
use axum::{
    response::{Html, IntoResponse},
    routing::get,
    Json, Router,
};
use luffy_common::util;
use serde::Serialize;

// View model for the page
#[derive(Debug, Serialize)]
pub struct StatusViewModel {
    // Vehicle state
    pub vehicle_id: String,
    pub location: String,
    pub yaw: f32,
    pub battery: f32,
    pub armed: bool,
    pub flight_mode: String,

    // Server statuses
    pub server_status: String,
    pub mavlink_connected: bool,
    pub iot_connected: bool,
    pub broker_connected: bool,

    // Add version field
    pub version: String,
}

impl From<VehicleState> for StatusViewModel {
    fn from(state: VehicleState) -> Self {
        Self {
            vehicle_id: util::get_vehicle_id(&CONFIG.base),
            location: format!("{:.6}, {:.6}", state.location.0, state.location.1),
            yaw: state.yaw_degree,
            battery: state.battery_percentage,
            armed: state.armed,
            flight_mode: state.flight_mode,

            server_status: "Running".to_string(),
            mavlink_connected: true, // Replace with actual status
            iot_connected: true,     // Replace with actual status
            broker_connected: true,  // Replace with actual status

            // Add version
            version: env::var("VERSION").unwrap_or_else(|_| "unknown".to_string()),
        }
    }
}

// Page template
#[derive(Template)]
#[template(path = "index.html")]
struct IndexPage {
    status: StatusViewModel,
}

pub fn routes(vehicle: &'static Vehicle) -> Router {
    Router::new()
        .route("/", get(move || index_page(vehicle)))
        .route("/api/status", get(move || status_api(vehicle)))
}

async fn index_page(vehicle: &'static Vehicle) -> impl IntoResponse {
    let template = IndexPage {
        status: StatusViewModel::from(vehicle.get_state_snapshot().unwrap_or_default()),
    };
    Html(template.render().unwrap())
}

async fn status_api(vehicle: &'static Vehicle) -> impl IntoResponse {
    Json(StatusViewModel::from(
        vehicle.get_state_snapshot().unwrap_or_default(),
    ))
}
