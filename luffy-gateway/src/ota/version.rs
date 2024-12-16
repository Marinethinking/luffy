use std::sync::{atomic::AtomicBool, Arc};

use anyhow::{anyhow, Result};
use luffy_common::ota::deb::ServiceType;
use luffy_common::ota::version::BaseVersionManager;
use luffy_gateway::config::CONFIG;
use tracing::{info, warn};

#[derive(Clone)]
pub struct VersionManager {
    base: BaseVersionManager,
    running: Arc<AtomicBool>,
}

impl VersionManager {
    pub fn new() -> Self {
        Self {
            base: BaseVersionManager::new(CONFIG.ota.clone().into()),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn check_updates(&self) -> Result<Vec<(String, String)>> {
        let (_, all_packages) = self.base.get_latest_version().await?;

        // Filter launcher packages that need updates
        let updates = all_packages
            .into_iter()
            .filter(|(filename, _)| filename.starts_with("luffy-launcher"))
            .filter(|(filename, _)| {
                if let Some(new_version) = self.base.deb_manager.extract_package_version(filename) {
                    self.base
                        .deb_manager
                        .needs_update("luffy-launcher", &new_version)
                        .unwrap_or(false)
                } else {
                    false
                }
            })
            .collect();

        Ok(updates)
    }

    pub async fn check_and_apply_updates(&self) -> Result<()> {
        match self.base.strategy.as_str() {
            "auto" => {
                let updates = self.check_updates().await?;
                if !updates.is_empty() {
                    self.update_launcher(updates).await?;
                }
                Ok(())
            }
            "manual" => {
                let updates = self.check_updates().await?;
                if !updates.is_empty() {
                    info!("Launcher updates available: {:?}", updates);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    async fn update_launcher(&self, packages: Vec<(String, String)>) -> Result<()> {
        let service_type = ServiceType::Other("luffy-launcher".to_string());
        self.base
            .update_service_packages(&service_type, &packages)
            .await
    }

    pub fn stop(&self) {
        self.running.store(false, std::sync::atomic::Ordering::Relaxed);
    }

    pub async fn start(&self) -> Result<()> {
        let mut interval = tokio::time::interval(self.base.check_interval);
        let manager = self.clone();

        self.running.store(true, std::sync::atomic::Ordering::Relaxed);

        match self.base.strategy.as_str() {
            "auto" => {
                info!(
                    "Starting auto update task with interval: {:?}",
                    self.base.check_interval
                );

                while self.running.load(std::sync::atomic::Ordering::Relaxed) {
                    interval.tick().await;
                    if let Err(e) = manager.check_and_apply_updates().await {
                        warn!("Auto update check failed: {}", e);
                    }
                }
                Ok(())
            }
            "manual" => {
                info!(
                    "Starting manual update check with interval: {:?}",
                    self.base.check_interval
                );

                while self.running.load(std::sync::atomic::Ordering::Relaxed) {
                    interval.tick().await;
                    match manager.check_updates().await {
                        Ok(updates) => {
                            if !updates.is_empty() {
                                let update_info: Vec<_> = updates
                                    .iter()
                                    .filter_map(|(filename, _)| {
                                        let new_version = self
                                            .base
                                            .deb_manager
                                            .extract_package_version(filename)?;
                                        let current_version = self
                                            .base
                                            .deb_manager
                                            .get_package_version("luffy-launcher")
                                            .ok()?;
                                        Some(("luffy-launcher", current_version, new_version))
                                    })
                                    .collect();

                                info!(
                                    "Launcher update available: {}",
                                    update_info
                                        .iter()
                                        .map(|(pkg, curr, new)| format!(
                                            "{}: {} -> {}",
                                            pkg, curr, new
                                        ))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                );
                            }
                        }
                        Err(e) => warn!("Manual update check failed: {}", e),
                    }
                }
                Ok(())
            }
            _ => {
                info!("Updates are disabled");
                Ok(())
            }
        }
    }
}
