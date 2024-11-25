use anyhow::anyhow;
use anyhow::{Context, Result};
use serde::Serialize;
use std::sync::{Arc, RwLock};
use tokio::sync::OnceCell;

use crate::util;

static VEHICLE: OnceCell<Vehicle> = OnceCell::const_new();

#[derive(Debug, Clone, Serialize)]
pub struct VehicleState {
    // Flight data
    pub yaw_degree: f32,
    pub pitch_degree: f32,
    pub roll_degree: f32,
    pub altitude: f32,
    pub battery_percentage: f32,
    pub gps_position: (f64, f64), // (latitude, longitude)
    pub armed: bool,
    pub flight_mode: String,

    // System status
    pub last_heartbeat: std::time::SystemTime,
    pub errors: Vec<String>,
}

impl Default for VehicleState {
    fn default() -> Self {
        Self {
            yaw_degree: 0.0,
            pitch_degree: 0.0,
            roll_degree: 0.0,
            altitude: 0.0,
            battery_percentage: 0.0,
            gps_position: (0.0, 0.0),
            armed: false,
            flight_mode: "STABILIZE".to_string(),
            last_heartbeat: std::time::SystemTime::now(),
            errors: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub struct Vehicle {
    pub device_id: String,
    state: Arc<RwLock<VehicleState>>,
}

impl Vehicle {
    pub async fn instance() -> &'static Self {
        VEHICLE
            .get_or_init(|| async {
                Self {
                    device_id: util::get_device_mac(),
                    state: Arc::new(RwLock::new(VehicleState::default())),
                }
            })
            .await
    }

    // Getters and setters for vehicle state
    pub fn update_attitude(&self, yaw: f32, pitch: f32, roll: f32) -> Result<()> {
        let mut state = self
            .state
            .write()
            .map_err(|e| anyhow!("Lock error: {}", e))?;
        state.yaw_degree = yaw;
        state.pitch_degree = pitch;
        state.roll_degree = roll;
        Ok(())
    }

    pub fn update_battery(&self, percentage: f32) -> Result<()> {
        let mut state = self
            .state
            .write()
            .map_err(|e| anyhow!("Lock error: {}", e))?;
        state.battery_percentage = percentage;
        Ok(())
    }

    pub fn get_state_snapshot(&self) -> Result<VehicleState> {
        let state = self
            .state
            .read()
            .map_err(|e| anyhow!("Lock error: {}", e))?;
        Ok(state.clone())
    }

    // Example of a more complex operation
    pub fn update_flight_mode(&self, mode: String) -> Result<()> {
        let mut state = self
            .state
            .write()
            .map_err(|e| anyhow!("Lock error: {}", e))?;
        state.flight_mode = mode;
        // Log mode change or perform additional actions
        Ok(())
    }
}
