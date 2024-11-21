use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::vehicle::Vehicle;
use anyhow::Result;

pub struct MavlinkServer {
    vehicle: &'static Vehicle,
    running: Arc<AtomicBool>,
}

impl MavlinkServer {
    pub async fn new() -> Self {
        Self {
            vehicle: Vehicle::instance().await,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn start(&self) -> Result<()> {
        while self.running.load(Ordering::SeqCst) {
            // Read MAVLink messages and update vehicle state
            if let Ok(state) = self.vehicle.get_state_snapshot() {
                self.vehicle.update_attitude(
                    state.yaw_degree,
                    state.pitch_degree,
                    state.roll_degree,
                )?;
            }

            // Update other vehicle states...
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Ok(())
    }

    pub async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}
