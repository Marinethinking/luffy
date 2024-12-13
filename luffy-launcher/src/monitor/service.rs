use std::collections::HashMap;

#[derive(Default, Clone, Debug)]
pub struct Services {
    pub services: HashMap<String, ServiceState>,
}

#[derive(Clone, Debug)]
pub struct ServiceState {
    pub name: String,
    pub status: ServiceStatus,
    pub last_health_report: std::time::SystemTime,
}

#[derive(Clone, Debug)]
pub enum ServiceStatus {
    Unknown,
    Running,
    Stopped,
}

impl Services {
    pub fn new() -> Self {
        Self {
            services: HashMap::new(),
        }
    }

    pub fn set_service(&mut self, name: &str, status: ServiceStatus) {
        self.services.insert(
            name.to_string(),
            ServiceState {
                name: name.to_string(),
                status,
                last_health_report: std::time::SystemTime::now(),
            },
        );
    }
}
