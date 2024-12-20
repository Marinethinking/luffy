use crate::config::CFG;
use crate::monitor::mqtt::MQTT_MONITOR;
use anyhow::{anyhow, Result};
use luffy_common::ota::deb::ServiceType;
use luffy_common::ota::version::BaseVersionManager;
use std::collections::HashMap;
use std::sync::{atomic::AtomicBool, Arc};
use tracing::{info, warn};

#[derive(Clone)]
pub struct VersionManager {
    base: BaseVersionManager,
    running: Arc<AtomicBool>,
}

impl Default for VersionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl VersionManager {
    pub fn new() -> Self {
        Self {
            base: BaseVersionManager::new(CFG.ota.clone().into()),
            running: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn get_current_version(&self) -> &str {
        self.base.get_current_version()
    }

    pub async fn get_latest_version(&self) -> Result<(String, Vec<(String, String)>)> {
        self.base.get_latest_version().await
    }

    pub async fn manual_update(&self, service: &str) -> Result<()> {
        let (_, packages) = self.get_latest_version().await?;
        let service_packages: Vec<(String, String)> = packages
            .into_iter()
            .filter(|(filename, _)| filename.contains(service))
            .collect();

        if service_packages.is_empty() {
            return Err(anyhow!("No updates found for {}", service));
        }

        self.update_package(service_packages).await
    }

    pub async fn update_package(&self, packages: Vec<(String, String)>) -> Result<()> {
        let service_packages: Vec<(String, String)> = packages
            .into_iter()
            .filter(|(filename, _)| !filename.starts_with("luffy-launcher"))
            .filter(|(filename, _)| {
                if let Some(package_name) = filename.split('_').next() {
                    if let Some(new_version) =
                        self.base.deb_manager.extract_package_version(filename)
                    {
                        self.base
                            .deb_manager
                            .needs_update(package_name, &new_version)
                            .unwrap_or(false)
                    } else {
                        false
                    }
                } else {
                    false
                }
            })
            .collect();

        if service_packages.is_empty() {
            info!("No service updates to process");
            return Ok(());
        }
        info!("Updates available: {:?}", service_packages);

        let mut updates_by_service: HashMap<ServiceType, Vec<(String, String)>> = HashMap::new();
        for package in service_packages {
            let service_type = self.base.deb_manager.get_service_type(&package.0);
            updates_by_service
                .entry(service_type)
                .or_default()
                .push(package);
        }

        for (service_type, packages) in &updates_by_service {
            if let Err(e) = self
                .base
                .update_service_packages(service_type, packages)
                .await
            {
                warn!("Failed to update {:?}: {}", service_type, e);
                return Err(e);
            }
        }

        info!("Successfully updated all services");
        Ok(())
    }

    pub async fn check_updates(&self) -> Result<Vec<(String, String)>> {
        let (_, all_packages) = self.get_latest_version().await?;

        // Filter packages that need updates
        let updates: Vec<(String, String)> = all_packages
            .clone()
            .into_iter()
            .filter(|(filename, _)| !filename.starts_with("luffy-launcher"))
            .filter(|(filename, _)| {
                if let Some(package_name) = filename.split('_').next() {
                    if let Some(new_version) =
                        self.base.deb_manager.extract_package_version(filename)
                    {
                        self.base
                            .deb_manager
                            .needs_update(package_name, &new_version)
                            .unwrap_or(false)
                    } else {
                        false
                    }
                } else {
                    false
                }
            })
            .collect();
        self.set_latest_version(all_packages).await;
        info!("Found updates {:?}", updates);
        Ok(updates)
    }

    async fn set_latest_version(&self, packages: Vec<(String, String)>) {
        let monitor = MQTT_MONITOR.get().unwrap();
        let mut services = monitor.services.write().await;
        for (package, _) in packages {
            let service = self.base.deb_manager.get_service_type(&package);
            let version = self.base.deb_manager.extract_package_version(&package);
            services.set_service(&service.to_string(), None, None, version);
        }
    }

    pub async fn check_and_apply_updates(&self) -> Result<()> {
        match self.base.strategy.as_str() {
            "auto" => {
                let updates = self.check_updates().await?;
                if !updates.is_empty() {
                    self.update_package(updates).await?;
                }
                Ok(())
            }
            "manual" => {
                let updates = self.check_updates().await?;
                if !updates.is_empty() {
                    info!("Updates available: {:?}", updates);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub fn stop(&self) {
        self.running
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    pub async fn start(&self) -> Result<()> {
        let mut interval = tokio::time::interval(self.base.check_interval);
        let manager = self.clone();

        self.running
            .store(true, std::sync::atomic::Ordering::Relaxed);

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
                                        let package_name = filename.split('_').next()?;
                                        let new_version = self
                                            .base
                                            .deb_manager
                                            .extract_package_version(filename)?;
                                        let current_version = self
                                            .base
                                            .deb_manager
                                            .get_package_version(package_name)
                                            .ok()?;
                                        Some((package_name, current_version, new_version))
                                    })
                                    .collect();

                                info!(
                                    "Updates available: {}",
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
