use crate::ota::deb::{DebManager, ServiceType};
use anyhow::{anyhow, Context, Result};
use reqwest;

use semver::Version;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::time::Duration;
use tracing::{info, warn};

#[derive(Debug, Deserialize)]
pub struct GithubRelease {
    pub tag_name: String,
    pub assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
pub struct GithubAsset {
    pub name: String,
    pub browser_download_url: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VersionConfig {
    pub strategy: String,
    pub check_interval: u32,
    pub download_dir: Option<String>,
    pub github_repo: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BaseVersionManager {
    pub strategy: String,
    pub current_version: String,
    pub check_interval: Duration,
    pub deb_manager: DebManager,
    pub github_repo: String,
}

impl BaseVersionManager {
    pub fn new(config: VersionConfig) -> Self {
        let work_dir = PathBuf::from(
            config
                .download_dir
                .unwrap_or("/home/luffy/.deb".to_string()),
        );

        Self {
            strategy: config.strategy,
            current_version: String::new(),
            check_interval: Duration::from_secs(config.check_interval as u64),
            deb_manager: DebManager::new(work_dir),
            github_repo: config.github_repo,
        }
    }

    pub fn get_current_version(&self) -> &str {
        &self.current_version
    }

    pub async fn get_latest_version(&self) -> Result<(String, Vec<(String, String)>)> {
        let client = reqwest::Client::new();
        let url = format!(
            "https://api.github.com/repos/{}/releases/latest",
            self.github_repo
        );

        info!("Requesting GitHub releases from: {}", url);

        let mut request = client
            .get(&url)
            .header("User-Agent", "luffy-updater")
            .header("Accept", "application/vnd.github.v3+json");

        if let Ok(token) = std::env::var("LUFFY_GITHUB_TOKEN") {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        let response = request.send().await.context("Failed to fetch releases")?;
        let release: GithubRelease = response.json().await?;

        let deb_assets: Vec<(String, String)> = release
            .assets
            .iter()
            .filter(|asset| asset.name.ends_with(".deb"))
            .map(|asset| (asset.name.clone(), asset.browser_download_url.clone()))
            .collect();
        info!("Latest version: {}", release.tag_name);
        Ok((release.tag_name, deb_assets))
    }

    pub async fn update_service_packages(
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

        info!("Successfully updated {:?}", service_type);
        Ok(())
    }

    pub async fn needs_update(&self, latest_version: &str) -> Result<bool> {
        let current = Version::parse(&self.current_version)?;
        let latest = Version::parse(latest_version.trim_start_matches('v'))?;
        Ok(latest > current)
    }
}
