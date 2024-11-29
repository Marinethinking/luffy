use anyhow::Result;
use luffy::ota::{
    update::OtaUpdater,
    version::{BoatMode, SubscriptionType, VehicleInfo, VersionManager},
};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    let subscriber = FmtSubscriber::builder().with_max_level(Level::INFO).init();

    info!("Starting OTA test...");

    // Initialize test vehicle info
    let vehicle_info = VehicleInfo {
        id: "test-device-001".to_string(),
        subscription: SubscriptionType::Basic,
        boat_mode: BoatMode::Manual,
        current_version: "0.1.0".to_string(),
    };

    info!("Testing with vehicle info: {:?}", vehicle_info);

    // Create version manager
    let version_manager = VersionManager::new(vehicle_info)?;

    let release_info = version_manager.check_update_availability().await?;

    if let None = release_info {
        info!("No updates available");
        return Ok(());
    }

    let release_info = release_info.unwrap();

    info!("Update available: {:?}", release_info);

    // Initialize updater
    let updater = OtaUpdater::new("luffy")?;

    // Create backup
    let backup_path = updater
        .create_backup(&version_manager.get_current_version().to_string())
        .await?;
    info!("Backup created at: {:?}", backup_path);

    // Download update
    let update_path = updater.download_update(&release_info.version).await?;
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
