use luffy_launcher::launcher::service_manager::ServiceManager;

use luffy_launcher::ota::OtaManager;
use luffy_launcher::web::server::WebServer;
use std::path::PathBuf;
use tokio::signal;
use tokio::sync::broadcast;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let environment = std::env::var("RUST_ENV").unwrap_or_else(|_| "development".to_string());
    let config_path = PathBuf::from("luffy-deploy/config")
        .join(&environment)
        .join("launcher.toml");

    // Create a shutdown signal channel
    let (shutdown_tx, _) = broadcast::channel(1);

    // Spawn service manager task with shutdown signal
    let service_handle = spawn_service_manager(shutdown_tx.subscribe()).await;

    // Spawn OTA checker task with shutdown signal
    let ota_handle = spawn_ota_checker(shutdown_tx.subscribe()).await;

    // Spawn web console task with shutdown signal
    let web_handle = spawn_web_server(shutdown_tx.subscribe()).await;

    // Handle shutdown signal
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

    // Wait for all tasks to complete
    let results = tokio::join!(service_handle, ota_handle, web_handle, shutdown_signal);

    // Check for errors
    for (result, name) in [results.0, results.1, results.2].into_iter().zip([
        "Service manager",
        "OTA checker",
        "Web console",
    ]) {
        if let Err(e) = result {
            error!("{} join error: {}", name, e);
        }
    }

    Ok(())
}

async fn spawn_service_manager(
    mut shutdown: broadcast::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    let manager = ServiceManager {};
    tokio::spawn(async move {
        tokio::select! {
            result = manager.start_services() => {
                if let Err(e) = result {
                    error!("Service manager error: {}", e);
                }
            }
            _ = shutdown.recv() => {
                info!("Shutting down service manager...");
                // Add stop method to ServiceManager if needed
                // manager.stop().await;
            }
        }
    })
}

async fn spawn_ota_checker(mut shutdown: broadcast::Receiver<()>) -> tokio::task::JoinHandle<()> {
    let ota = OtaManager::new();
    tokio::spawn(async move {
        tokio::select! {
            _ = ota.check_updates() => {}
            _ = shutdown.recv() => {
                info!("Shutting down OTA checker...");
                // Add stop method to OtaManager if needed
                // ota.stop().await;
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
                // Add stop method to WebServer if needed
                // web.stop().await;
            }
        }
    })
}
