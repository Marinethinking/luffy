use crate::config::CONFIG;
use anyhow::{anyhow, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::{fs, path::PathBuf};
use std::io::Write;
use futures_util::StreamExt;
use tracing::{info, warn};
use crate::ota::version::*;

pub struct OtaUpdater {
    backup_path: PathBuf,
    service_name: String,
}

impl OtaUpdater {
    pub fn new(service_name: &str) -> Result<Self> {
        let backup_path = std::env::current_exe()?
            .parent()
            .ok_or_else(|| anyhow!("Cannot get parent directory"))?
            .join("backup");

        if !backup_path.exists() {
            fs::create_dir_all(&backup_path)?;
        }

        Ok(Self {
            backup_path,
            service_name: service_name.to_string(),
        })
    }

    pub async fn download_update(&self, version: &str) -> Result<PathBuf> {
        let filename = format!("luffy_{}-1_arm64.deb", version.trim_start_matches('v'));
        let url = format!("{}/{}/{}", RELEASE_URL, version, filename);
        
        info!("Downloading update from {}", url);
        
        let temp_path = self.backup_path.join(&filename);
        
        let response = reqwest::get(&url).await?;
        let total_size = response.content_length().unwrap_or(0);

        let pb = ProgressBar::new(total_size);
        pb.set_style(ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("#>-"));

        let mut file = fs::File::create(&temp_path)?;
        let mut downloaded: u64 = 0;
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk)?;
            downloaded = std::cmp::min(downloaded + (chunk.len() as u64), total_size);
            pb.set_position(downloaded);
        }

        pb.finish_with_message("Download complete");
        Ok(temp_path)
    }

    pub async fn create_backup(&self, version: &str) -> Result<PathBuf> {
        let current_exe = std::env::current_exe()?;
        let backup_file = self.backup_path.join(format!("backup_v{}", version));
        
        info!("Creating backup at: {:?}", backup_file);
        fs::copy(&current_exe, &backup_file)?;
        
        Ok(backup_file)
    }

    pub async fn apply_update(&self, update_path: &PathBuf) -> Result<()> {
        let current_exe = std::env::current_exe()?;
        
        self.stop_service().await?;
        fs::copy(update_path, &current_exe)?;
        
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&current_exe, fs::Permissions::from_mode(0o755))?;
        }
        
        self.start_service().await?;
        Ok(())
    }

    pub async fn rollback(&self, backup_path: &PathBuf) -> Result<()> {
        if !backup_path.exists() {
            return Err(anyhow!("Backup file not found"));
        }

        self.stop_service().await?;
        let current_exe = std::env::current_exe()?;
        fs::copy(backup_path, &current_exe)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&current_exe, fs::Permissions::from_mode(0o755))?;
        }

        self.start_service().await?;
        Ok(())
    }

    async fn stop_service(&self) -> Result<()> {
        info!("Stopping service: {}", self.service_name);
        #[cfg(target_os = "linux")]
        {
            let output = std::process::Command::new("systemctl")
                .args(["stop", &self.service_name])
                .output()?;
            if !output.status.success() {
                return Err(anyhow!("Failed to stop service"));
            }
        }
        Ok(())
    }

    async fn start_service(&self) -> Result<()> {
        info!("Starting service: {}", self.service_name);
        #[cfg(target_os = "linux")]
        {
            let output = std::process::Command::new("systemctl")
                .args(["start", &self.service_name])
                .output()?;
            if !output.status.success() {
                return Err(anyhow!("Failed to start service"));
            }
        }
        Ok(())
    }

    pub async fn cleanup_old_backups(&self, keep_versions: usize) -> Result<()> {
        let mut backups: Vec<_> = fs::read_dir(&self.backup_path)?
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("backup_v"))
            .collect();

        if backups.len() <= keep_versions {
            return Ok(());
        }

        backups.sort_by_key(|entry| entry.metadata().unwrap().modified().unwrap());
        for backup in backups.iter().take(backups.len() - keep_versions) {
            if let Err(e) = fs::remove_file(backup.path()) {
                warn!("Failed to remove old backup {:?}: {}", backup.path(), e);
            }
        }
        Ok(())
    }
}
