use anyhow::Result;

use luffy_gateway::broker::MqttBroker;
use luffy_gateway::config::CONFIG;
use luffy_gateway::iot::server::IotServer;
use luffy_gateway::mav_server::MavlinkServer;

use tokio::signal;
use tokio::sync::broadcast;
use tracing::{error, info};

use luffy_gateway::ota::version::VersionManager;

#[tokio::main]
async fn main() -> Result<()> {
    let log_level = &CONFIG.log_level;
    luffy_common::util::setup_logging(log_level, "gateway");
    info!("Application starting...");

    info!("Region: {:?}", &CONFIG.base.aws.region);

    // Create a shutdown signal channel
    let (shutdown_tx, _) = broadcast::channel(1);

    // Spawn all services
    let mav_handle = if CONFIG.feature.mavlink {
        spawn_mavlink_server(shutdown_tx.subscribe()).await
    } else {
        info!("MAVLink server disabled in config, skipping...");
        tokio::spawn(async {})
    };

    let broker_handle = if CONFIG.feature.broker {
        spawn_mqtt_broker(shutdown_tx.subscribe()).await
    } else {
        info!("MQTT broker disabled in config, skipping...");
        tokio::spawn(async {})
    };

    let iot_handle = if CONFIG.feature.local_iot || CONFIG.feature.remote_iot {
        spawn_iot_server(shutdown_tx.subscribe()).await
    } else {
        info!("IoT server disabled in config, skipping...");
        tokio::spawn(async {})
    };

    let ota_handle = if CONFIG.ota.enable {
        spawn_ota_server(shutdown_tx.subscribe()).await
    } else {
        info!("OTA server disabled in config, skipping...");
        tokio::spawn(async {})
    };

    let shutdown_signal = async {
        match signal::ctrl_c().await {
            Ok(()) => {
                info!("Shutdown signal received, stopping services...");
                shutdown_tx
                    .send(())
                    .expect("Failed to send shutdown signal");
            }
            Err(err) => {
                error!("Failed to listen for shutdown signal: {}", err);
            }
        }
    };

    let results = tokio::join!(
        mav_handle,
        iot_handle,
        broker_handle,
        ota_handle,
        shutdown_signal
    );

    for (result, name) in [results.0, results.1, results.2, results.3]
        .into_iter()
        .zip(["MAVLink server", "IoT server", "MQTT broker", "OTA server"])
    {
        if let Err(e) = result {
            error!("{} join error: {}", name, e);
        }
    }

    info!("All services stopped, shutting down");

    Ok(())
}

async fn spawn_mqtt_broker(mut shutdown: broadcast::Receiver<()>) -> tokio::task::JoinHandle<()> {
    info!("Starting MQTT broker...");
    let mut broker = MqttBroker::new().await;
    tokio::spawn(async move {
        tokio::select! {
            result = broker.start() => {
                if let Err(e) = result {
                    error!("MQTT broker error: {}", e);
                }
            }
            _ = shutdown.recv() => {
                info!("Shutting down MQTT broker...");
                broker.stop().await;
            }
        }
    })
}

async fn spawn_mavlink_server(
    mut shutdown: broadcast::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    let mut mav_server = MavlinkServer::new().await;
    tokio::spawn(async move {
        tokio::select! {
            result = mav_server.start() => {
                if let Err(e) = result {
                    error!("MAVLink server error: {}", e);
                }
            }
            _ = shutdown.recv() => {
                info!("Shutting down MAVLink server...");
                mav_server.stop().await;
            }
        }
    })
}

async fn spawn_iot_server(mut shutdown: broadcast::Receiver<()>) -> tokio::task::JoinHandle<()> {
    info!("Starting IoT server...");
    let mut server = IotServer::new().await;
    tokio::spawn(async move {
        tokio::select! {
            result = server.start() => {
                if let Err(e) = result {
                    error!("IoT server error: {}", e);
                }
            }
            _ = shutdown.recv() => {
                info!("Shutting down IoT server...");
                server.stop().await;
            }
        }
    })
}

async fn spawn_ota_server(mut shutdown: broadcast::Receiver<()>) -> tokio::task::JoinHandle<()> {
    info!("Starting OTA server...");
    let version_manager = VersionManager::new();
    tokio::spawn(async move {
        tokio::select! {
            result = version_manager.start() => {
                if let Err(e) = result {
                    error!("Version manager error: {}", e);
                }
            }
            _ = shutdown.recv() => {
                info!("Shutting down OTA server...");
                version_manager.stop();
            }
        }
    })
}
