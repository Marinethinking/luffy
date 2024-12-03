use crate::config::CONFIG;
use crate::ota::update::OtaUpdater;
use anyhow::{anyhow, Result};
use reqwest;
use semver::Version;
use serde::{Deserialize, Serialize};
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};

pub const GITHUB_API_URL: &str = "https://api.github.com/repos/Marinethinking/luffy/releases";
pub const RELEASE_URL: &str = "https://github.com/Marinethinking/luffy/releases/download";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum UpgradeStrategy {
    Auto,     // Automatically check and upgrade
    Manual,   // Wait for upstream command
    Disabled, // No upgrades allowed
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VersionManager {
    strategy: UpgradeStrategy,
    current_version: String,
    check_interval: Duration,
}

impl VersionManager {
    pub fn new() -> Result<Self> {
        Ok(Self {
            strategy: CONFIG.ota.strategy.clone(),
            current_version: env!("CARGO_PKG_VERSION").to_string(),
            check_interval: Duration::from_secs(CONFIG.ota.check_interval),
        })
    }

    pub async fn start_version_management(&self) -> Result<()> {
        match self.strategy {
            UpgradeStrategy::Auto => {
                self.start_auto_update_task().await?;
            }
            UpgradeStrategy::Manual => {
                info!("Manual update mode - waiting for upstream commands");
            }
            UpgradeStrategy::Disabled => {
                info!("Version upgrades are disabled");
            }
        }
        Ok(())
    }

    async fn start_auto_update_task(&self) -> Result<()> {
        info!(
            "Starting auto update task with interval: {:?}",
            self.check_interval
        );
        let mut interval = interval(self.check_interval);
        let manager = self.clone();

        tokio::spawn(async move {
            loop {
                interval.tick().await;
                if let Err(e) = manager.check_and_apply_updates().await {
                    warn!("Auto update check failed: {}", e);
                }
            }
        });

        Ok(())
    }

    pub async fn get_latest_version(&self) -> Result<String> {
        let client = reqwest::Client::new();
        let response = client
            .get(GITHUB_API_URL)
            .header("User-Agent", "luffy-updater")
            .send()
            .await?;

        let releases: Vec<GitHubRelease> = response.json().await?;

        let latest = releases
            .into_iter()
            .find(|r| !r.draft && !r.prerelease)
            .ok_or_else(|| anyhow!("No releases found"))?;

        Ok(latest.tag_name.trim_start_matches('v').to_string())
    }

    async fn check_and_apply_updates(&self) -> Result<()> {
        // TODO: Implement subscription verification with upstream service
        // For now, we'll proceed without verification

        let latest_version = self.get_latest_version().await?;
        let current = Version::parse(&self.current_version)?;
        let latest = Version::parse(&latest_version)?;

        if latest > current {
            info!("New version available: {} -> {}", current, latest);

            let updater = OtaUpdater::new("luffy")?;
            let backup_path = updater.create_backup(&self.current_version).await?;

            match updater.download_update(&latest_version).await {
                Ok(update_path) => {
                    if let Err(e) = updater.apply_update(&update_path).await {
                        warn!("Update failed, rolling back: {}", e);
                        if let Err(e) = updater.rollback(&backup_path).await {
                            error!("Rollback failed: {}", e);
                        }
                    } else {
                        info!("Update successful");
                        if let Err(e) = updater
                            .cleanup_old_backups(CONFIG.ota.backup_count as usize)
                            .await
                        {
                            warn!("Failed to cleanup old backups: {}", e);
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to download update: {}", e);
                }
            }
        } else {
            info!("Already running the latest version {}", current);
        }

        Ok(())
    }

    pub fn get_current_version(&self) -> &str {
        &self.current_version
    }
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
}
