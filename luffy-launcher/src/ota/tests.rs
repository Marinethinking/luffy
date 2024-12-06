#[cfg(test)]
mod ota_tests {
    use super::super::version::VersionManager;
    use anyhow::Result;
    use tracing_subscriber::{fmt, EnvFilter};

    fn init_logger() {
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::from_default_env().add_directive(tracing::Level::DEBUG.into()),
            )
            .with_test_writer()
            .init();
    }

    #[tokio::test]
    async fn test_version_management() -> Result<()> {
        let version_manager = VersionManager::new();

        // Check current version
        let current = version_manager.get_current_version();
        assert!(!current.is_empty());
        println!("Current version: {}", current);

        // Check latest version from GitHub
        let (latest_version, download_url) = version_manager.get_latest_version().await?;
        assert!(!latest_version.is_empty());
        assert!(download_url.ends_with(".deb"));
        println!("Latest version: {} ({})", latest_version, download_url);

        Ok(())
    }

    #[tokio::test]
    async fn test_deb_update() -> Result<()> {
        init_logger();
        let version_manager = VersionManager::new();

        // Test the update process
        version_manager.check_and_apply_updates().await?;

        Ok(())
    }
}
