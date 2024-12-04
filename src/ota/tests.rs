#[cfg(test)]
mod ota_tests {
    use super::super::version::VersionManager;
    use anyhow::Result;

    #[tokio::test]
    async fn test_version_management() -> Result<()> {
        let version_manager = VersionManager::new()?;

        // Check current version
        let current = version_manager.get_current_version();
        assert!(!current.is_empty());
        println!("Current version: {}", current);

        // Check latest version from Docker Hub
        let latest = version_manager.get_latest_version().await?;
        assert!(!latest.is_empty());
        println!("Latest version: {}", latest);

        Ok(())
    }

    #[tokio::test]
    async fn test_docker_update() -> Result<()> {
        let version_manager = VersionManager::new()?;

        // Test the update process
        version_manager.check_and_apply_updates().await?;

        Ok(())
    }
}
