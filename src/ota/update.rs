use crate::config::CONFIG;
use anyhow::{anyhow, Result};

use crate::aws_client::AwsClient;
use indicatif::{ProgressBar, ProgressStyle};
use std::process::Command;
use std::{fs, path::PathBuf};
use tracing::{info, warn};

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

        let download_path = self
            .backup_path
            .join(format!("download_v{}.tar.gz", version));

        // Get AWS client instance
        let aws_client = AwsClient::instance().await;

        // Construct the correct S3 key
        let s3_key = format!(
            "{}/luffy-{}-{}",
            CONFIG.ota.release_path, version, "aarch64"
        );

        // Get object size first
        let head_object = aws_client
            .s3()
            .head_object()
            .bucket(&CONFIG.ota.s3_bucket)
            .key(&s3_key)
            .send()
            .await?;

        let total_size = head_object.content_length().unwrap() as u64;

        // Create progress bar
        let pb = ProgressBar::new(total_size);
        pb.set_style(ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("#>-"));

        // Download with progress
        let mut response = aws_client
            .s3()
            .get_object()
            .bucket(&CONFIG.ota.s3_bucket)
            .key(&s3_key)
            .send()
            .await?;

        let mut file = fs::File::create(&download_path)?;
        let mut downloaded: u64 = 0;

        while let Some(chunk) = response.body.try_next().await? {
            downloaded += chunk.len() as u64;
            pb.set_position(downloaded);
            std::io::Write::write_all(&mut file, &chunk)?;
        }

        pb.finish_with_message("Download completed");

        if !download_path.exists() {
            return Err(anyhow!("Failed to download update"));
        }

        info!("Update downloaded successfully to {:?}", download_path);

        // Extract the package
        let extract_dir = self.backup_path.join(format!("update_v{}", version));
        fs::create_dir_all(&extract_dir)?;

        let status = Command::new("tar")
            .args(["xzf", &download_path.to_string_lossy()])
            .current_dir(&extract_dir)
            .status()?;

        if !status.success() {
            return Err(anyhow!("Failed to extract update package"));
        }

        // The extracted binary will be in a subdirectory
        let package_dir = fs::read_dir(&extract_dir)?
            .filter_map(Result::ok)
            .find(|entry| entry.file_name().to_string_lossy().contains("luffy-"))
            .ok_or_else(|| anyhow!("Cannot find extracted package directory"))?;

        Ok(package_dir.path())
    }

    pub async fn create_backup(&self, version: &str) -> Result<PathBuf> {
        let current_exe = std::env::current_exe()?;
        let backup_file = self.backup_path.join(format!("backup_v{}", version));

        info!("Creating backup at: {:?}", backup_file);
        fs::copy(&current_exe, &backup_file)?;

        Ok(backup_file)
    }

    pub async fn apply_update(&self, update_dir: &PathBuf) -> Result<()> {
        let current_exe = std::env::current_exe()?;
        let update_binary = update_dir.join("luffy");
        let update_config = update_dir.join("config");

        // Stop the service
        self.stop_service().await?;

        // Replace the executable
        fs::copy(update_binary, &current_exe)?;

        // Update config files if they exist
        if update_config.exists() {
            let config_dir = current_exe
                .parent()
                .ok_or_else(|| anyhow!("Cannot get parent directory"))?
                .join("config");

            if !config_dir.exists() {
                fs::create_dir_all(&config_dir)?;
            }

            // Copy config files
            for entry in fs::read_dir(update_config)? {
                let entry = entry?;
                let dest = config_dir.join(entry.file_name());
                fs::copy(entry.path(), dest)?;
            }
        }

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
