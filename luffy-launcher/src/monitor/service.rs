use serde::Deserialize;
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use tracing::info;

#[derive(Default, Clone, Debug)]
pub struct Services {
    pub services: HashMap<String, ServiceState>,
}

#[derive(Clone, Debug)]
pub struct ServiceState {
    pub name: String,
    pub status: ServiceStatus,
    pub last_health_report: std::time::SystemTime,
    pub version: String,
    pub latest_version: Option<String>,
}

#[derive(Clone, Debug)]
pub enum ServiceStatus {
    Unknown,
    Running,
    Stopped,
}

#[derive(Debug, Deserialize)]
pub struct HealthReport {
    pub version: String,
}

impl Services {
    pub fn new() -> Self {
        let mut services = Self {
            services: HashMap::new(),
        };

        // Initialize all known services with Unknown status
        for service in ["gateway", "launcher", "media"] {
            services.set_service(
                service,
                Some(ServiceStatus::Unknown),
                Some("Unknown".to_string()),
                None,
            );
        }

        services
    }

    pub fn set_service(
        &mut self,
        name: &str,
        status: Option<ServiceStatus>,
        version: Option<String>,
        latest_version: Option<String>,
    ) {
        info!(
            "Setting service {} to status {:?}, version {:?}, latest_version {:?}",
            name, status, version, latest_version
        );
        let service_name = name.to_lowercase();
        if let Some(service) = self.services.get_mut(&service_name) {
            if let Some(status) = status {
                service.status = status;
            }
            if let Some(version) = version {
                service.version = version;
            }
            if let Some(latest_version) = latest_version {
                service.latest_version = Some(latest_version);
            }
            service.last_health_report = std::time::SystemTime::now();
        } else {
            self.services.insert(
                name.to_string(),
                ServiceState {
                    name: name.to_string(),
                    status: status.unwrap_or(ServiceStatus::Unknown),
                    last_health_report: std::time::SystemTime::now(),
                    version: version.unwrap_or("Unknown".to_string()),
                    latest_version,
                },
            );
        }
    }

    pub fn get_service_status(&self, name: &str) -> ServiceStatus {
        if let Some(service) = self.services.get(name) {
            let elapsed = SystemTime::now()
                .duration_since(service.last_health_report)
                .unwrap_or(Duration::from_secs(61));

            if elapsed.as_secs() > 60 {
                ServiceStatus::Unknown
            } else {
                service.status.clone()
            }
        } else {
            ServiceStatus::Unknown
        }
    }
}
