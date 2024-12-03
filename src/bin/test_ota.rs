use anyhow::Result;
use luffy::ota::{
    update::OtaUpdater,
    version::{SubscriptionType, UpgradeStrategy, VersionManager},
};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    let _subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .init();

    info!("Starting OTA test...");

    // Create version manager
    let version_manager = VersionManager::new()?;
    info!("Version manager initialized with current version: {}", version_manager.get_current_version());

    // Test auto-update strategy
    info!("Testing auto-update strategy...");
    version_manager.start_version_management().await?;

    // Initialize updater
    let updater = OtaUpdater::new("luffy")?;

    // Check for updates
    let latest_version = version_manager.get_latest_version().await?;
    info!("Latest version: {}", latest_version);

    // Create backup
    let backup_path = updater
        .create_backup(&version_manager.get_current_version())
        .await?;
    info!("Backup created at: {:?}", backup_path);

    // Download update
    let update_path = updater.download_update(&latest_version).await?;
    info!("Update downloaded to: {:?}", update_path);

    // Apply update
    updater.apply_update(&update_path).await?;
    info!("Update applied successfully");

    // Cleanup old backups
    updater.cleanup_old_backups(2).await?;
    info!("Cleaned up old backups");

    // Optional: Test rollback
    // info!("Testing rollback...");
    // updater.rollback(&backup_path).await?;
    // info!("Rollback completed successfully");

    info!("OTA test completed successfully");
    Ok(())
}
