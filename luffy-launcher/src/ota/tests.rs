#[cfg(test)]
mod ota_tests {
    use crate::config::CFG;

    use super::super::version::VersionManager;

    use anyhow::Result;
    use std::env;
    use tracing::info;

    fn init() {
        env::set_var("RUST_ENV", "dev");

        luffy_common::util::setup_logging("debug", "luffy-launcher");
    }

    #[tokio::test]
    async fn test_version_management() -> Result<()> {
        let version_manager = VersionManager::new();

        // Check current version
        let current = version_manager.get_current_version();
        assert!(!current.is_empty());
        println!("Current version: {}", current);

        // Check latest version from GitHub
        let (latest_version, packages) = version_manager.get_latest_version().await?;
        assert!(!latest_version.is_empty());
        assert!(!packages.is_empty());

        println!("Latest version: {}", latest_version);
        for (filename, url) in packages {
            println!("Package: {} ({})", filename, url);
            assert!(filename.ends_with(".deb"));
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_deb_update() -> Result<()> {
        init();
        info!("Starting test_deb_update {}", CFG.ota.strategy);
        let version_manager = VersionManager::new();

        // Test the update process
        version_manager.check_and_apply_updates().await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_check_updates() -> Result<()> {
        init();
        let version_manager = VersionManager::new();
        version_manager.check_updates().await?;
        Ok(())
    }
}
