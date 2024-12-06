use crate::config::CONFIG;
use crate::ota::deb::{DebManager, ServiceType};
use anyhow::{anyhow, Context, Result};
use reqwest;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
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
    deb_manager: DebManager,
}

impl Default for VersionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl VersionManager {
    pub fn new() -> Self {
        let work_dir = PathBuf::from(
            CONFIG
                .ota
                .download_dir
                .clone()
                .unwrap_or("/home/luffy/.deb".to_string()),
        );

        Self {
            strategy: CONFIG.ota.strategy.clone(),
            current_version: env!("CARGO_PKG_VERSION").to_string(),
            check_interval: Duration::from_secs(CONFIG.ota.check_interval as u64),
            deb_manager: DebManager::new(work_dir),
        }
    }

    pub async fn get_latest_version(&self) -> Result<(String, Vec<(String, String)>)> {
        let client = reqwest::Client::new();
        let url = format!(
            "https://api.github.com/repos/{}/releases/latest",
            CONFIG.ota.github_repo
        );

        info!("Requesting GitHub releases from: {}", url);

        let mut request = client
            .get(&url)
            .header("User-Agent", "luffy-updater")
            .header("Accept", "application/vnd.github.v3+json");

        // Add authorization token from environment variable
        if let Ok(token) = std::env::var("LUFFY_GITHUB_TOKEN") {
            request = request.header("Authorization", format!("Bearer {}", token));
        } else {
            warn!("LUFFY_GITHUB_TOKEN not found in environment variables");
        }

        let response = request.send().await.context("Failed to fetch releases")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "Could not read error response".to_string());

            return Err(anyhow!(
                "GitHub API request failed. Status: {}, Body: {}",
                status,
                error_body
            ));
        }

        let release: GithubRelease = response.json().await?;

        // Filter and collect all .deb assets
        let deb_assets: Vec<(String, String)> = release
            .assets
            .iter()
            .filter(|asset| {
                asset.name.ends_with(".deb") && {
                    let package_name = asset.name.split('_').next().unwrap_or("");
                    self.should_update_package(package_name)
                }
            })
            .map(|asset| (asset.name.clone(), asset.browser_download_url.clone()))
            .collect();

        if deb_assets.is_empty() {
            return Err(anyhow!("No applicable .deb packages found in release"));
        }

        Ok((release.tag_name, deb_assets))
    }

    pub async fn update_package(&self, packages: Vec<(String, String)>) -> Result<()> {
        // Filter out launcher updates
        let service_packages: Vec<(String, String)> = packages
            .into_iter()
            .filter(|(filename, _)| !filename.starts_with("luffy-launcher"))
            .collect();

        if service_packages.is_empty() {
            info!("No service updates to process");
            return Ok(());
        }

        // Group packages by service type
        let mut updates_by_service: HashMap<ServiceType, Vec<(String, String)>> = HashMap::new();
        for package in service_packages {
            let service_type = self.deb_manager.get_service_type(&package.0);
            updates_by_service
                .entry(service_type)
                .or_default()
                .push(package);
        }

        // Process updates service by service
        for (service_type, packages) in &updates_by_service {
            if let Err(e) = self.update_service(service_type, packages).await {
                warn!("Failed to update {:?}: {}", service_type, e);
                return Err(e);
            }
        }

        info!("Successfully updated all services");
        Ok(())
    }

    async fn update_service(
        &self,
        service_type: &ServiceType,
        packages: &[(String, String)],
    ) -> Result<()> {
        info!("Processing updates for {:?}", service_type);

        // Download packages
        let mut downloaded_files = Vec::new();
        for (filename, url) in packages {
            info!("Downloading {} from {}", filename, url);
            match self.deb_manager.download_deb(url, filename).await {
                Ok(path) => downloaded_files.push(path),
                Err(e) => {
                    for path in downloaded_files {
                        let _ = tokio::fs::remove_file(path).await;
                    }
                    return Err(e);
                }
            }
        }

        // Stop service
        if let Err(e) = self.deb_manager.stop_service(service_type).await {
            warn!("Failed to stop {:?}: {}", service_type, e);
        }

        // Install packages
        let mut install_failed = false;
        for deb_path in &downloaded_files {
            if !self.deb_manager.install_package(deb_path).await? {
                install_failed = true;
                break;
            }
        }

        if install_failed {
            warn!("Update failed for {:?}, attempting rollback", service_type);
            // Try to rollback using last installed version
            for (filename, _) in packages {
                let package_name = filename.split('_').next().unwrap_or("");
                if !self
                    .deb_manager
                    .install_from_last_installed(package_name)
                    .await?
                {
                    warn!("Rollback failed for {}", package_name);
                }
            }
            return Err(anyhow!("Service update failed"));
        }

        // Start service
        if let Err(e) = self.deb_manager.start_service(service_type).await {
            warn!("Failed to start {:?}: {}", service_type, e);
        }

        Ok(())
    }

    pub async fn check_and_apply_updates(&self) -> Result<()> {
        let (latest_version, packages) = self.get_latest_version().await?;

        let current = Version::parse(&self.current_version)?;
        let latest = Version::parse(latest_version.trim_start_matches('v'))?;

        if latest > current {
            info!("New version available: {} -> {}", current, latest);
            match self.update_package(packages).await {
                Ok(_) => info!("Update successful - services will be restarted by dpkg"),
                Err(e) => warn!("Update failed: {}", e),
            }
        } else {
            info!("Already running the latest version {}", current);
        }
        Ok(())
    }

    pub async fn start(&self) -> Result<()> {
        if luffy_common::util::is_dev() {
            info!("Skipping auto update in dev mode");
            return Ok(());
        }

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

    fn should_update_package(&self, package_name: &str) -> bool {
        match package_name {
            "luffy-gateway" => CONFIG.ota.gateway,
            "luffy-media" => CONFIG.ota.media,
            _ => false,
        }
    }
}
