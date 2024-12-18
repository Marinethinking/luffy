use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use std::path::PathBuf;
use std::process::Command;
use strum_macros::Display;
use tokio::fs;

use tracing::{info, warn};

use semver::Version;

#[derive(Debug, Hash, Eq, PartialEq, Clone, Display)]
pub enum ServiceType {
    Gateway,
    Media,
    Launcher,
    Other(String),
}
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DebManager {
    work_dir: PathBuf,
}

impl DebManager {
    pub fn new(work_dir: PathBuf) -> Self {
        Self { work_dir }
    }

    pub async fn download_deb(&self, url: &str, filename: &str) -> Result<PathBuf> {
        // Ensure work directory exists
        fs::create_dir_all(&self.work_dir).await?;

        // Save current version if it exists
        let package_name = filename.split('_').next().unwrap_or("");
        if let Ok(current_version) = self.get_installed_version(package_name).await {
            let backup_filename = format!("{}_{}_{}", package_name, current_version, "backup.deb");
            let backup_path = self.work_dir.join(&backup_filename);

            // Copy current deb to backup location if it exists
            if let Ok(current_deb) = self.find_current_deb(package_name).await {
                fs::copy(current_deb, &backup_path).await?;
            }
        }

        // Download new version
        let deb_path = self.work_dir.join(filename);
        let response = reqwest::get(url).await?;
        let bytes = response.bytes().await?;
        fs::write(&deb_path, bytes).await?;

        Ok(deb_path)
    }

    async fn get_sorted_package_files(
        &self,
        package_name: &str,
        suffix: &str,
    ) -> Result<Vec<tokio::fs::DirEntry>> {
        let mut files = Vec::new();
        let mut entries = fs::read_dir(&self.work_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(package_name) && name.ends_with(suffix) {
                files.push(entry);
            }
        }

        files.sort_by(|a, b| {
            let a_time = futures::executor::block_on(a.metadata())
                .and_then(|m| m.modified())
                .ok();
            let b_time = futures::executor::block_on(b.metadata())
                .and_then(|m| m.modified())
                .ok();
            b_time.cmp(&a_time)
        });

        Ok(files)
    }

    async fn find_current_deb(&self, package_name: &str) -> Result<PathBuf> {
        let backups = self
            .get_sorted_package_files(package_name, "backup.deb")
            .await?;
        backups
            .first()
            .map(|entry| entry.path())
            .ok_or_else(|| anyhow!("No backup found for {}", package_name))
    }

    pub async fn cleanup_old_files(&self, package_name: &str, keep_count: usize) -> Result<()> {
        let files = self.get_sorted_package_files(package_name, ".deb").await?;

        for entry in files.iter().skip(keep_count) {
            fs::remove_file(entry.path()).await?;
        }

        Ok(())
    }

    pub async fn get_installed_version(&self, package_name: &str) -> Result<String> {
        let output = Command::new("dpkg-query")
            .args(["-W", "-f=${Version}", package_name])
            .output()
            .context(format!("Failed to get version for {}", package_name))?;

        if !output.status.success() {
            return Err(anyhow!("Package {} not found", package_name));
        }

        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    }

    pub async fn install_package(&self, deb_path: &PathBuf) -> Result<bool> {
        info!("Installing package {:?}", deb_path);
        let package_name = deb_path
            .file_name()
            .and_then(|f| f.to_str())
            .and_then(|s| s.split('_').next())
            .ok_or_else(|| anyhow!("Invalid package filename"))?;

        let status = Command::new("sudo")
            .args(["dpkg", "-i"])
            .arg(deb_path.to_str().unwrap())
            .status()
            .context("Failed to install package")?;

        if status.success() {
            // Mark as installed and cleanup other files
            self.mark_as_installed(deb_path).await?;
            self.cleanup_package_files(package_name).await?;
            info!("Installed package {:?}", deb_path);
            Ok(true)
        } else {
            info!("Failed to install package {:?}", deb_path);
            Ok(false)
        }
    }

    pub async fn install_from_last_installed(&self, package_name: &str) -> Result<bool> {
        if let Ok(last_installed) = self.find_last_installed(package_name).await {
            warn!(
                "Installing from last known good version: {:?}",
                last_installed
            );
            self.install_package(&last_installed).await
        } else {
            warn!("No previous installed version found for {}", package_name);
            Ok(false)
        }
    }

    pub async fn rollback_package(&self, package_name: &str, version: &str) -> Result<()> {
        info!("Rolling back {} to version {}", package_name, version);

        // Find the backup .deb file for this version
        let backup_filename = format!("{}_{}_{}", package_name, version, "backup.deb");
        let backup_path = self.work_dir.join(&backup_filename);

        if !backup_path.exists() {
            return Err(anyhow!("Backup file not found for version {}", version));
        }

        let status = Command::new("sudo")
            .args(["dpkg", "-i"])
            .arg(backup_path.to_str().unwrap())
            .status()
            .context(format!("Failed to rollback {}", package_name))?;

        if !status.success() {
            return Err(anyhow!("Failed to rollback package"));
        }
        info!("Rolled back {} to version {}", package_name, version);
        Ok(())
    }

    pub async fn stop_service(&self, service_type: &ServiceType) -> Result<()> {
        let service_name = self.get_service_name(service_type);

        #[cfg(target_os = "linux")]
        {
            Command::new("sudo")
                .args(["systemctl", "stop", &service_name])
                .status()
                .context("Failed to stop service")?;
        }
        #[cfg(not(target_os = "linux"))]
        {
            warn!("Service control is only supported on Linux systems");
        }
        info!("Stopped service {:?}", service_type);
        Ok(())
    }

    pub async fn start_service(&self, service_type: &ServiceType) -> Result<()> {
        let service_name = self.get_service_name(service_type);

        #[cfg(target_os = "linux")]
        {
            Command::new("sudo")
                .args(["systemctl", "start", &service_name])
                .status()
                .context("Failed to start service")?;
        }
        #[cfg(not(target_os = "linux"))]
        {
            warn!("Service control is only supported on Linux systems");
        }
        info!("Started service {:?}", service_type);
        Ok(())
    }

    pub fn get_service_name(&self, service_type: &ServiceType) -> String {
        match service_type {
            ServiceType::Gateway => "luffy-gateway".to_string(),
            ServiceType::Media => "luffy-media".to_string(),
            ServiceType::Launcher => "luffy-launcher".to_string(),
            ServiceType::Other(name) => name.clone(),
        }
    }

    pub fn get_service_type(&self, package_name: &str) -> ServiceType {
        match package_name {
            name if name.starts_with("luffy-gateway") => ServiceType::Gateway,
            name if name.starts_with("luffy-media") => ServiceType::Media,
            name if name.starts_with("luffy-launcher") => ServiceType::Launcher,
            name => ServiceType::Other(name.to_string()),
        }
    }

    async fn mark_as_installed(&self, deb_path: &PathBuf) -> Result<PathBuf> {
        let filename = deb_path
            .file_name()
            .and_then(|f| f.to_str())
            .ok_or_else(|| anyhow!("Invalid deb path"))?;

        let installed_name = filename.replace(".deb", "_installed.deb");
        let installed_path = self.work_dir.join(&installed_name);

        fs::rename(deb_path, &installed_path).await?;
        info!("Marked as installed: {:?}", installed_path);
        Ok(installed_path)
    }

    async fn find_last_installed(&self, package_name: &str) -> Result<PathBuf> {
        let files = self
            .get_sorted_package_files(package_name, "_installed.deb")
            .await?;
        info!("Found installed files: {:?}", files);
        files
            .first()
            .map(|entry| entry.path())
            .ok_or_else(|| anyhow!("No installed version found for {}", package_name))
    }

    async fn cleanup_package_files(&self, package_name: &str) -> Result<()> {
        let mut entries = fs::read_dir(&self.work_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(package_name) && !name.ends_with("_installed.deb") {
                fs::remove_file(entry.path()).await?;
            }
        }
        info!("Cleaned up package files for {}", package_name);
        Ok(())
    }

    pub fn is_package_installed(&self, package_name: &str) -> Result<bool> {
        match Command::new("dpkg").arg("-l").arg(package_name).output() {
            Ok(output) => Ok(output.status.success()),
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    warn!("dpkg command not found. System might not be Debian-based");
                    Ok(false)
                } else {
                    Err(anyhow!("Failed to check package installation: {}", e))
                }
            }
        }
    }

    pub fn extract_package_version(&self, filename: &str) -> Option<String> {
        // Format: package-name_version_arch.deb
        filename.split('_').nth(1).map(|s| s.to_string())
    }

    pub fn get_package_version(&self, package_name: &str) -> Result<String> {
        match Command::new("dpkg-query")
            .args(["-W", "-f=${Version}", package_name])
            .output()
        {
            Ok(output) => {
                if output.status.success() {
                    Ok(String::from_utf8(output.stdout)?.trim().to_string())
                } else {
                    Err(anyhow!("Package {} not found", package_name))
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Err(anyhow!(
                        "dpkg-query command not found. System might not be Debian-based"
                    ))
                } else {
                    Err(anyhow!("Failed to get package version: {}", e))
                }
            }
        }
    }

    pub fn needs_update(&self, package_name: &str, new_version: &str) -> Result<bool> {
        if let Ok(current_version) = self.get_package_version(package_name) {
            if let (Ok(current), Ok(new)) = (
                Version::parse(&current_version),
                Version::parse(new_version),
            ) {
                info!("Current version: {}, new version: {}", current, new);
                return Ok(new > current);
            }
        }
        Ok(false)
    }
}
