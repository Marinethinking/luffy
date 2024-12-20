use anyhow::Result;

use tracing::info;

use luffy_media::config::CONFIG;
use luffy_media::media::service::MEDIA_SERVICE;
use luffy_media::mqtt::MQTT_HANDLER;
use luffy_media::ws::WS_SERVER;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    let log_level = &CONFIG.log_level;
    luffy_common::util::setup_logging(log_level, "media");
    info!("Starting luffy-media...");

    // Create media server
    let media_task = tokio::spawn(async move { MEDIA_SERVICE.start().await });

    let mqtt_task = tokio::spawn(async move { MQTT_HANDLER.start().await });

    let ws_task = tokio::spawn(async move { WS_SERVER.start().await });

    // Join all tasks and handle any errors
    let (media_result, mqtt_result, ws_result) = tokio::join!(media_task, mqtt_task, ws_task);

    // Handle results
    media_result.unwrap()?;
    mqtt_result.unwrap()?;
    ws_result.unwrap()?;

    // Keep the main thread alive
    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");

    Ok(())
}
