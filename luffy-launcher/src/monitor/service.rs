use serde::Deserialize;
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

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
            services.set_service(service, ServiceStatus::Unknown, "Unknown".to_string());
        }

        services
    }

    pub fn set_service(&mut self, name: &str, status: ServiceStatus, version: String) {
        self.services.insert(
            name.to_string(),
            ServiceState {
                name: name.to_string(),
                status,
                last_health_report: std::time::SystemTime::now(),
                version,
            },
        );
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
