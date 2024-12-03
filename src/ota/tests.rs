#[cfg(test)]
mod ota_tests {
    use super::super::version::VersionManager;
    use anyhow::Result;

    #[tokio::test]
    async fn test_ota_update_flow() -> Result<()> {
        let version_manager = VersionManager::new()?;
        assert!(!version_manager.get_current_version().is_empty());

        let latest_version = version_manager.get_latest_version().await?;
        println!("Latest version: {}", latest_version);
        assert!(!latest_version.is_empty());

        version_manager.update_container(&latest_version).await?;
        Ok(())
    }
}
