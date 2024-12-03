#[cfg(test)]
mod ota_tests {
    use super::super::{update::OtaUpdater, version::VersionManager};
    use anyhow::Result;

    #[tokio::test]
    async fn test_ota_update_flow() -> Result<()> {
        // Create version manager
        let version_manager = VersionManager::new()?;
        assert!(!version_manager.get_current_version().is_empty());

        // Initialize updater
        let updater = OtaUpdater::new("luffy")?;

        // Check for updates
        let latest_version = version_manager.get_latest_version().await?;
        println!("Latest version: {}", latest_version);
        assert!(!latest_version.is_empty());

        // Create backup
        let backup_path = updater
            .create_backup(version_manager.get_current_version())
            .await?;
        println!("Backup path: {}", backup_path.display());
        assert!(backup_path.exists());

        // Download update
        let update_path = updater.download_update(&latest_version).await?;
        println!("Update path: {}", update_path.display());
        assert!(update_path.exists());

        // Cleanup old backups
        updater.cleanup_old_backups(2).await?;
        println!("Cleanup old backups");
        Ok(())
    }

    // Add more specific test cases for individual components
    #[tokio::test]
    async fn test_version_check() -> Result<()> {
        let manager = VersionManager::new()?;
        let latest = manager.get_latest_version().await?;
        assert!(!latest.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn test_backup_creation() -> Result<()> {
        let updater = OtaUpdater::new("luffy")?;
        let backup_path = updater.create_backup("0.1.0").await?;
        assert!(backup_path.exists());
        Ok(())
    }
}
