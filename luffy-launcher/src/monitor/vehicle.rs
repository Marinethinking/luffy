use serde::{Deserialize, Serialize};
use std::time::SystemTime;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VehicleState {
    // Flight data
    pub yaw_degree: f32,
    pub pitch_degree: f32,
    pub roll_degree: f32,
    pub altitude: f32,
    pub battery_percentage: f32,
    pub location: (f64, f64), // (latitude, longitude)
    pub armed: bool,
    pub flight_mode: String,

    // System status
    pub last_heartbeat: SystemTime,
    pub errors: Vec<String>,
    pub luffy: String,
}

impl Default for VehicleState {
    fn default() -> Self {
        Self {
            yaw_degree: 0.0,
            pitch_degree: 0.0,
            roll_degree: 0.0,
            altitude: 0.0,
            battery_percentage: 0.0,
            location: (0.0, 0.0),
            armed: false,
            flight_mode: "MANUAL".to_string(),
            last_heartbeat: SystemTime::now(),
            errors: Vec::new(),
            luffy: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}
