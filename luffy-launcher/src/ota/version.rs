use crate::config::CONFIG;
use anyhow::{anyhow, Context, Result};
use reqwest;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use tokio::fs;
use tokio::time::{interval, Duration};
use tracing::{info, warn};

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VersionManager {
    strategy: String,
    current_version: String,
    check_interval: Duration,
    temp_dir: PathBuf,
}

impl Default for VersionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl VersionManager {
    pub fn new() -> Self {
        Self {
            strategy: CONFIG.ota.strategy.clone(),
            current_version: env!("CARGO_PKG_VERSION").to_string(),
            check_interval: Duration::from_secs(CONFIG.ota.check_interval as u64),
            temp_dir: std::env::temp_dir().join("luffy-updates"),
        }
    }

    async fn download_deb(&self, url: &str, version: &str) -> Result<PathBuf> {
        // Create temp directory if it doesn't exist
        fs::create_dir_all(&self.temp_dir).await?;

        let deb_path = self.temp_dir.join(format!("luffy-{}.deb", version));

        // Download the file
        let response = reqwest::get(url).await?;
        let bytes = response.bytes().await?;
        fs::write(&deb_path, bytes).await?;

        Ok(deb_path)
    }

    pub async fn get_latest_version(&self) -> Result<(String, String)> {
        let client = reqwest::Client::new();
        let url = format!(
            "https://api.github.com/repos/{}/releases/latest",
            CONFIG.ota.github_repo
        );

        let mut request = client.get(&url).header("User-Agent", "luffy-updater");

        // Add authorization token if provided
        if let Some(token) = &CONFIG.ota.github_token {
            request = request.header("Authorization", format!("token {}", token));
        }

        let response = request.send().await.context("Failed to fetch releases")?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "GitHub API request failed with status: {}",
                response.status()
            ));
        }

        let release: GithubRelease = response.json().await?;
        
        // Find the .deb asset
        let deb_asset = release
            .assets
            .iter()
            .find(|asset| asset.name.ends_with(".deb"))
            .ok_or_else(|| anyhow!("No .deb package found in release"))?;

        Ok((release.tag_name.clone(), deb_asset.browser_download_url.clone()))
    }

    pub async fn update_package(&self, version: &str, url: &str) -> Result<()> {
        info!("Downloading new version {} from {}", version, url);
        let deb_path = self.download_deb(url, version).await?;

        info!("Installing new package");
        let status = Command::new("sudo")
            .args(["dpkg", "-i"])
            .arg(deb_path.to_str().unwrap())
            .status()
            .context("Failed to install package")?;

        if !status.success() {
            return Err(anyhow!("Package installation failed"));
        }

        // Clean up downloaded file
        fs::remove_file(deb_path).await?;

        Ok(())
    }

    pub async fn check_and_apply_updates(&self) -> Result<()> {
        let (latest_version, download_url) = self.get_latest_version().await?;
        
        let current = Version::parse(&self.current_version)?;
        let latest = Version::parse(latest_version.trim_start_matches('v'))?;

        if latest > current {
            info!("New version available: {} -> {}", current, latest);
            match self.update_package(&latest_version, &download_url).await {
                Ok(_) => {
                    info!("Update successful");
                    // Restart the service
                    Command::new("sudo")
                        .args(["systemctl", "restart", &CONFIG.ota.service_name])
                        .status()
                        .context("Failed to restart service")?;
                }
                Err(e) => warn!("Update failed: {}", e),
            }
        } else {
            info!("Already running the latest version {}", current);
        }
        Ok(())
    }

    pub async fn start(&self) -> Result<()> {
        match self.strategy.as_str() {
            "auto" => {
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
            "manual" => info!("Manual update mode - waiting for upstream commands"),
            "disabled" => info!("Version upgrades are disabled"),
            _ => return Err(anyhow!("Invalid upgrade strategy: {}", self.strategy)),
        }
        Ok(())
    }

    pub fn get_current_version(&self) -> &str {
        &self.current_version
    }
}
