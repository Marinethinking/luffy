use anyhow::Result;

use luffy::broker::MqttBroker;
use luffy::config::{Config, CONFIG};
use luffy::iot::server::IotServer;
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
    setup_logging();
    info!("Application starting...");

    info!("Region: {:?}", &CONFIG.aws.region);

    // Create a shutdown signal channel
    let (shutdown_tx, _) = broadcast::channel(1);

    // Initialize services
    let mav_server = MavlinkServer::new().await;
    let iot_server = IotServer::new().await;
    let web_server = WebServer::new().await;
    let mqtt_broker = MqttBroker::new().await;

    // Spawn all services
    let mav_handle = spawn_mavlink_server(mav_server, shutdown_tx.subscribe()).await;

    let web_handle = spawn_web_server(web_server, shutdown_tx.subscribe()).await;
    let mqtt_handle = if CONFIG.rumqttd.enabled {
        info!("Starting MQTT broker...");
        spawn_mqtt_broker(mqtt_broker, shutdown_tx.subscribe()).await
    } else {
        info!("MQTT broker disabled in config, skipping...");
        tokio::spawn(async {})
    };
    info!("Starting IoT server...");
    let iot_handle = spawn_iot_server(iot_server, shutdown_tx.subscribe()).await;

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
        web_handle,
        mqtt_handle,
        shutdown_signal
    );

    for (result, name) in [results.0, results.1, results.2, results.3]
        .into_iter()
        .zip(["MAVLink server", "IoT server", "Web server", "MQTT broker"])
    {
        if let Err(e) = result {
            error!("{} join error: {}", name, e);
        }
    }

    info!("All services stopped, shutting down");

    Ok(())
}

async fn spawn_mqtt_broker(
    mut broker: MqttBroker,
    mut shutdown: broadcast::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
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
    server: MavlinkServer,
    mut shutdown: broadcast::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
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
    })
}

async fn spawn_iot_server(
    mut server: IotServer,
    mut shutdown: broadcast::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
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
    })
}

async fn spawn_web_server(
    server: WebServer,
    mut shutdown: broadcast::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
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
    })
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
                .add_directive(Level::DEBUG.into())
                .add_directive("tokio=debug".parse().unwrap()) // Tokio runtime logs
                .add_directive("runtime=debug".parse().unwrap()),
        ) // Runtime events
        .try_init()
        .expect("Failed to initialize logging");
}
