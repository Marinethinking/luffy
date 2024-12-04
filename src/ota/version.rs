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
    count: u32,
    next: Option<String>,
    previous: Option<String>,
    results: Vec<DockerTag>,
}

#[derive(Debug, Deserialize)]
struct DockerTag {
    name: String,
    last_updated: String,
    tag_status: String,
    // We can add other fields if needed, but these are the essential ones
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
            .args(["pull", &format!("{}:{}", CONFIG.ota.image_name, version)])
            .status()?;
        Ok(())
    }

    pub async fn get_latest_version(&self) -> Result<String> {
        let client = reqwest::Client::new();

        let response = client
            .get(&CONFIG.ota.version_check_url)
            .header("User-Agent", "luffy-updater")
            .send()
            .await
            .map_err(|e| {
                warn!("Failed to send request: {}", e);
                e
            })?;

        if !response.status().is_success() {
            warn!("Request failed with status: {}", response.status());
            return Err(anyhow!(
                "HTTP request failed with status: {}",
                response.status()
            ));
        }

        let body = response.text().await.map_err(|e| {
            warn!("Failed to get response body: {}", e);
            e
        })?;

        let tags: DockerHubResponse = serde_json::from_str(&body).map_err(|e| {
            warn!("Failed to parse JSON: {} - Response: {}", e, body);
            anyhow!("JSON parsing error: {}", e)
        })?;

        let latest = tags
            .results
            .into_iter()
            .filter(|t| {
                t.name != "latest"
                    && t.tag_status == "active"
                    && Version::parse(&t.name.trim_start_matches('v')).is_ok()
            })
            .max_by(|a, b| {
                let ver_a = Version::parse(&a.name.trim_start_matches('v')).unwrap();
                let ver_b = Version::parse(&b.name.trim_start_matches('v')).unwrap();
                ver_a.cmp(&ver_b)
            })
            .ok_or_else(|| anyhow!("No valid version tags found"))?;

        Ok(latest.name.clone())
    }

    pub async fn check_and_apply_updates(&self) -> Result<()> {
        let latest_version = self.get_latest_version().await?;
        let current = Version::parse(&self.current_version)?;
        let latest_version_trimmed = latest_version.trim_start_matches('v');
        let latest = Version::parse(latest_version_trimmed)?;

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
