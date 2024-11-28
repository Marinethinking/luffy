use crate::config::CONFIG;
use anyhow::{anyhow, Result};

use std::{fs, path::PathBuf};
use tracing::{error, info, warn};

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
        info!("Downloading update version {}", version);

        let download_path = self.backup_path.join(format!("download_v{}", version));
        let file = fs::File::create(&download_path)?;
        let download_url = &format!("s3://{}/{}", CONFIG.ota.s3_bucket, CONFIG.ota.bin_name);
        self_update::Download::from_url(download_url)
            .set_header(reqwest::header::ACCEPT, "application/octet-stream".parse()?)
            .download_to(file)?;

        if !download_path.exists() {
            return Err(anyhow!("Failed to download update"));
        }

        Ok(download_path)
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

        // Stop the service
        self.stop_service().await?;

        // Replace the executable
        fs::copy(update_path, &current_exe)?;

        // Set executable permissions on Unix systems
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&current_exe, fs::Permissions::from_mode(0o755))?;
        }

        // Start the service
        self.start_service().await?;

        Ok(())
    }

    pub async fn rollback(&self, backup_path: &PathBuf) -> Result<()> {
        info!("Rolling back to backup at {:?}", backup_path);

        if !backup_path.exists() {
            return Err(anyhow!("Backup file not found"));
        }

        // Stop the service
        self.stop_service().await?;

        // Restore from backup
        let current_exe = std::env::current_exe()?;
        fs::copy(backup_path, &current_exe)?;

        // Set executable permissions on Unix systems
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&current_exe, fs::Permissions::from_mode(0o755))?;
        }

        // Start the service
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
