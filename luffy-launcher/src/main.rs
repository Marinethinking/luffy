use luffy_launcher::{
    config::CONFIG, monitor::mqtt::MqttMonitor, ota::version::VersionManager,
    web::server::WebServer,
};

use tokio::signal;
use tokio::sync::broadcast;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let log_level = &CONFIG.log_level;
    luffy_common::util::setup_logging(log_level, "launcher");
    info!("Application starting...");

    // Create a shutdown signal channel
    let (shutdown_tx, _) = broadcast::channel(1);

    // Spawn OTA checker task with shutdown signal
    let ota_handle = spawn_ota_checker(shutdown_tx.subscribe()).await;

    // Spawn web console task with shutdown signal
    let web_handle = spawn_web_server(shutdown_tx.subscribe()).await;

    let monitor_handle = spawn_monitor_server(shutdown_tx.subscribe()).await;

    // Handle shutdown signal
    let shutdown_signal = async {
        match signal::ctrl_c().await {
            Ok(()) => {
                info!("Shutdown signal received...");
                shutdown_tx
                    .send(())
                    .expect("Failed to send shutdown signal");
            }
            Err(err) => {
                error!("Failed to listen for shutdown signal: {}", err);
            }
        }
    };

    // Wait for all tasks to complete
    let results = tokio::join!(ota_handle, web_handle, monitor_handle, shutdown_signal);

    // Check for errors
    for (result, name) in [results.0, results.1, results.2].into_iter().zip([
        "OTA checker",
        "Web console",
        "MQTT monitor",
    ]) {
        if let Err(e) = result {
            error!("{} join error: {}", name, e);
        }
    }

    Ok(())
}

async fn spawn_ota_checker(mut shutdown: broadcast::Receiver<()>) -> tokio::task::JoinHandle<()> {
    let ota = VersionManager::new();
    tokio::spawn(async move {
        tokio::select! {
            result = ota.start() => {
                if let Err(e) = result {
                    error!("OTA checker error: {}", e);
                }
            }
            _ = shutdown.recv() => {
                info!("Shutting down OTA checker...");
                ota.stop();
            }
        }
    })
}

async fn spawn_web_server(mut shutdown: broadcast::Receiver<()>) -> tokio::task::JoinHandle<()> {
    let web = WebServer::new().await;
    tokio::spawn(async move {
        tokio::select! {
            _ = web.start() => {}
            _ = shutdown.recv() => {
                info!("Shutting down web console...");
            }
        }
    })
}

async fn spawn_monitor_server(
    mut shutdown: broadcast::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    let monitor = MqttMonitor::instance().await;
    tokio::spawn(async move {
        tokio::select! {
            result = monitor.start() => {
                if let Err(e) = result {
                    error!("MQTT monitor error: {}", e);
                }
            }
            _ = shutdown.recv() => {
                info!("Shutting down MQTT monitor...");
            }
        }
    })
}
