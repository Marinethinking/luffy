use crate::config::CONFIG;
use anyhow::{anyhow, Result};
use reqwest;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::process::Command;
use tokio::time::{interval, Duration};
use tracing::{info, warn};

#[derive(Debug, Deserialize)]
struct DockerHubResponse {
    results: Vec<DockerTag>,
}

#[derive(Debug, Deserialize)]
struct DockerTag {
    name: String,
}

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

    pub async fn update_container(&self, version: &str) -> Result<()> {
        Command::new("docker")
            .args(["pull", &format!("marinethinking/luffy:{}", version)])
            .status()?;
        Ok(())
    }

    pub async fn get_latest_version(&self) -> Result<String> {
        let client = reqwest::Client::new();
        let response = client
            .get(&CONFIG.ota.version_check_url)
            .header("User-Agent", "luffy-updater")
            .send()
            .await?;

        let tags: DockerHubResponse = response.json().await?;
        let latest = tags
            .results
            .iter()
            .find(|t| t.name != "latest" && Version::parse(&t.name).is_ok())
            .ok_or_else(|| anyhow!("No valid version tags found"))?;

        Ok(latest.name.clone())
    }

    async fn check_and_apply_updates(&self) -> Result<()> {
        let latest_version = self.get_latest_version().await?;
        let current = Version::parse(&self.current_version)?;
        let latest = Version::parse(&latest_version)?;

        if latest > current {
            info!("New version available: {} -> {}", current, latest);
            match self.update_container(&latest_version).await {
                Ok(_) => info!("Update successful"),
                Err(e) => warn!("Update failed: {}", e),
            }
        } else {
            info!("Already running the latest version {}", current);
        }
        Ok(())
    }

    pub async fn start_version_management(&self) -> Result<()> {
        match self.strategy {
            UpgradeStrategy::Auto => {
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
            }
            UpgradeStrategy::Manual => info!("Manual update mode - waiting for upstream commands"),
            UpgradeStrategy::Disabled => info!("Version upgrades are disabled"),
        }
        Ok(())
    }

    pub fn get_current_version(&self) -> &str {
        &self.current_version
    }
}
