use anyhow::Result;
use dotenv::dotenv;
use luffy::iot_server::IotServer;
use luffy::mav_server::MavlinkServer;
use luffy::web_server::WebServer;
use tokio::signal;
use tokio::sync::broadcast;
use tracing::{error, info, Level};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    // Setup logging and env
    dotenv().ok();
    setup_logging();
    info!("Application starting...");

    // Create a shutdown signal channel
    let (shutdown_tx, _) = broadcast::channel(1);

    // Initialize services
    let mav_server = MavlinkServer::new().await;
    let iot_server = IotServer::new().await;
    let web_server = WebServer::new().await;

    // Spawn all services
    let mav_handle = spawn_mavlink_server(mav_server, shutdown_tx.subscribe());
    let iot_handle = spawn_iot_server(iot_server, shutdown_tx.subscribe());
    let web_handle = spawn_web_server(web_server, shutdown_tx.subscribe());
    let _ = tokio::join!(mav_handle, iot_handle, web_handle);

    // Wait for shutdown signal
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

    info!("All services stopped, shutting down");

    Ok(())
}

async fn spawn_mavlink_server(
    server: MavlinkServer,
    mut shutdown: broadcast::Receiver<()>,
) -> Result<()> {
    tokio::spawn(async move {
        tokio::select! {
            result = server.start() => {
                if let Err(e) = result {
                    error!("MAVLink server error: {}", e);
                }
            }
            _ = shutdown.recv() => {
                info!("Shutting down MAVLink server...");
                server.stop().await;
            }
        }
    });
    Ok(())
}

async fn spawn_iot_server(
    mut server: IotServer,
    mut shutdown: broadcast::Receiver<()>,
) -> Result<()> {
    let handle = tokio::spawn(async move {
        info!("About to start IoT server in select! macro...");
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
    });
    handle.await?;
    Ok(())
}

async fn spawn_web_server(server: WebServer, mut shutdown: broadcast::Receiver<()>) -> Result<()> {
    tokio::spawn(async move {
        tokio::select! {
            result = server.start() => {
                if let Err(e) = result {
                    error!("Web server error: {}", e);
                }
            }
            _ = shutdown.recv() => {
                info!("Shutting down web server...");
                server.stop().await;
            }
        }
    });
    Ok(())
}

fn setup_logging() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_thread_ids(true) // Show thread IDs
                .with_thread_names(true) // Show thread names
                .with_target(true) // Show module path
                .with_file(true) // Show file name
                .with_line_number(true) // Show line numbers
                .pretty(),
        ) // Pretty printing
        .with(
            EnvFilter::from_default_env()
                .add_directive(Level::INFO.into())
                .add_directive("tokio=debug".parse().unwrap()) // Tokio runtime logs
                .add_directive("runtime=debug".parse().unwrap()),
        ) // Runtime events
        .try_init()
        .expect("Failed to initialize logging");
}
