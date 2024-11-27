use anyhow::{Context, Result};
use mavlink::{self, ardupilotmega::*, MavConnection, MavHeader};
use num_traits::FromPrimitive;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::config::CONFIG;
use crate::vehicle::Vehicle;
use mavlink::ardupilotmega::MavMode;

pub struct MavlinkServer {
    vehicle: &'static Vehicle,
    running: Arc<AtomicBool>,
    command_rx: mpsc::Receiver<MavCommand>,
    connection: Arc<Mutex<Option<Box<dyn MavConnection<MavMessage> + Send + Sync>>>>,
}

// Commands that can be sent to the vehicle
#[derive(Debug)]
pub enum MavCommand {
    Arm(bool),
    SetMode(String),
}

impl MavlinkServer {
    pub async fn new() -> Self {
        Self {
            vehicle: Vehicle::instance().await,
            running: Arc::new(AtomicBool::new(false)),
            command_rx: mpsc::channel(100).1,
            connection: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting MAVLink server...");
        let (command_tx, command_rx) = mpsc::channel(100);

        // Store command_tx in Vehicle for other components to send commands
        Vehicle::instance().await.set_command_sender(command_tx)?;

        info!("Connecting to vehicle {}", CONFIG.mavlink.connection_string);
        // Connect to the vehicle using MAVLink
        let connection = Arc::new(Mutex::new(Some(
            mavlink::connect(&CONFIG.mavlink.connection_string)
                .context("Failed to connect to MAVLink vehicle")?,
        )));
        self.command_rx = command_rx;
        self.connection = connection;
        self.running.store(true, Ordering::SeqCst);

        while self.running.load(Ordering::SeqCst) {
            tokio::select! {
                // Handle incoming MAVLink messages
                Ok(result) = tokio::task::spawn_blocking({
                    let connection = Arc::clone(&self.connection);
                    move || connection.lock().unwrap().as_mut().unwrap().recv()
                }) => {
                    if let Ok((header, message)) = result {
                        self.handle_mavlink_message(header, message).await?;
                    }
                }

                // Handle command requests
                Some(command) = self.command_rx.recv() => {
                    self.handle_command(command).await?;
                }
            }

            // tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Ok(())
    }

    async fn handle_mavlink_message(&self, header: MavHeader, message: MavMessage) -> Result<()> {
        match message {
            MavMessage::ATTITUDE(attitude) => {
                self.vehicle.update_attitude(
                    attitude.yaw.to_degrees(),
                    attitude.pitch.to_degrees(),
                    attitude.roll.to_degrees(),
                )?;
            }
            MavMessage::HEARTBEAT(heartbeat) => {
                // debug!("Heartbeat: {:?}", heartbeat);
                let armed = heartbeat
                    .base_mode
                    .contains(MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED);
                self.vehicle.update_armed_state(armed)?;

                let mode = RoverMode::from_u32(heartbeat.custom_mode).unwrap_or(RoverMode::DEFAULT);

                self.vehicle.update_flight_mode(format!("{:?}", mode))?;
            }
            MavMessage::GLOBAL_POSITION_INT(pos) => {
                self.vehicle.update_position(
                    pos.lat as f64 / 1e7,
                    pos.lon as f64 / 1e7,
                    pos.relative_alt as f32 / 1000.0,
                )?;
            }
            MavMessage::SYS_STATUS(status) => {
                self.vehicle
                    .update_battery(status.battery_remaining as f32)?;
            }
            _ => {} // Handle other message types as needed
        }
        Ok(())
    }

    async fn handle_command(&mut self, command: MavCommand) -> Result<()> {
        let message = match command {
            MavCommand::Arm(arm) => MavMessage::COMMAND_LONG(COMMAND_LONG_DATA {
                target_system: 1,
                target_component: 1,
                command: MavCmd::MAV_CMD_COMPONENT_ARM_DISARM,
                confirmation: 0,
                param1: if arm { 1.0 } else { 0.0 },
                param2: 0.0,
                param3: 0.0,
                param4: 0.0,
                param5: 0.0,
                param6: 0.0,
                param7: 0.0,
            }),
            MavCommand::SetMode(mode) => {
                MavMessage::COMMAND_LONG(COMMAND_LONG_DATA {
                    command: MavCmd::MAV_CMD_DO_SET_MODE,
                    param1: 1.0, // Custom mode
                    param2: mode.parse::<f32>().unwrap(),
                    ..Default::default()
                })
            } // Implement other commands...
        };

        self.connection
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .send(&mavlink::MavHeader::default(), &message)?;
        Ok(())
    }

    pub async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}
