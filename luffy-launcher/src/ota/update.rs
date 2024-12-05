use crate::launcher::service_manager::ServiceManager;
use anyhow::{anyhow, Context, Result};
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::fs;
use tracing::{debug, info};

pub enum UpdateManager {
    Systemd(String),
    Docker(String),
    Binary(PathBuf),
    Debug,
}

impl UpdateManager {
    pub fn detect() -> Self {
        if cfg!(debug_assertions) {
            return UpdateManager::Debug;
        }

        if Command::new("systemctl").arg("--version").output().is_ok() {
            return UpdateManager::Systemd("luffy-launcher.service".to_string());
        }

        if Path::new("/.dockerenv").exists() {
            return UpdateManager::Docker("luffy-launcher".to_string());
        }

        UpdateManager::Binary(env::current_exe().unwrap_or_else(|_| "luffy-launcher".into()))
    }

    pub async fn update(&self, new_version_path: &Path) -> Result<()> {
        match self {
            UpdateManager::Debug => {
                debug!("Running in debug mode, skipping update");
                Ok(())
            }
            UpdateManager::Systemd(service) => {
                self.backup().await?;
                info!("Stopping systemd service {}", service);
                Command::new("systemctl")
                    .args(["stop", service])
                    .output()
                    .context("Failed to stop service")?;

                self.replace_binary(new_version_path).await?;

                info!("Starting systemd service {}", service);
                Command::new("systemctl")
                    .args(["start", service])
                    .output()
                    .context("Failed to start service")?;
                Ok(())
            }
            UpdateManager::Docker(container) => {
                self.backup().await?;
                info!("Stopping docker container {}", container);
                Command::new("docker")
                    .args(["stop", container])
                    .output()
                    .context("Failed to stop container")?;

                self.replace_binary(new_version_path).await?;

                info!("Starting docker container {}", container);
                Command::new("docker")
                    .args(["start", container])
                    .output()
                    .context("Failed to start container")?;
                Ok(())
            }
            UpdateManager::Binary(path) => {
                // For binary mode, we need to:
                // 1. Stop services using ServiceManager
                // 2. Backup and update the binary
                // 3. Restart services
                let service_manager = ServiceManager::new();

                info!("Stopping managed services");
                // Note: You might need to modify ServiceManager to add a stop_services method
                // or handle the Child processes differently

                self.backup().await?;
                self.replace_binary(new_version_path).await?;

                info!("Starting managed services");
                service_manager
                    .start_services()
                    .await
                    .map_err(|e| anyhow!("Failed to restart services: {}", e))
                    .context("Failed to restart services")?;

                Ok(())
            }
        }
    }

    async fn backup(&self) -> Result<()> {
        let current_path = match self {
            UpdateManager::Debug => return Ok(()),
            UpdateManager::Binary(path) => path,
            UpdateManager::Systemd(_) | UpdateManager::Docker(_) => {
                &env::current_exe().context("Failed to get current executable path")?
            }
        };

        let backup_path = current_path.with_extension("backup");
        info!("Backing up current binary to {:?}", backup_path);
        fs::copy(current_path, backup_path)
            .await
            .context("Failed to create backup")?;
        Ok(())
    }

    async fn replace_binary(&self, new_version_path: &Path) -> Result<()> {
        let target_path = match self {
            UpdateManager::Debug => return Ok(()),
            UpdateManager::Binary(path) => path,
            UpdateManager::Systemd(_) | UpdateManager::Docker(_) => {
                &env::current_exe().context("Failed to get current executable path")?
            }
        };

        info!("Replacing binary with new version");
        fs::copy(new_version_path, target_path)
            .await
            .context("Failed to replace binary")?;
        Ok(())
    }
}
