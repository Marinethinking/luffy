use anyhow::{anyhow, Context, Result};
use std::path::Path;
use std::process::Command;
use tracing::{debug, info};

pub enum UpdateManager {
    Systemd,
    Debug,
}

impl UpdateManager {
    pub fn detect() -> Self {
        if cfg!(debug_assertions) {
            return UpdateManager::Debug;
        }
        UpdateManager::Systemd
    }

    pub async fn update(&self, new_version_path: &Path) -> Result<()> {
        match self {
            UpdateManager::Debug => {
                debug!("Running in debug mode, skipping update");
                Ok(())
            }
            UpdateManager::Systemd => {
                info!("Installing new package");
                let status = Command::new("sudo")
                    .args(["dpkg", "-i"])
                    .arg(new_version_path.to_str().unwrap())
                    .status()
                    .context("Failed to install package")?;

                if !status.success() {
                    return Err(anyhow!("Package installation failed"));
                }

                // dpkg will handle service restart automatically through the package scripts
                Ok(())
            }
        }
    }
}
