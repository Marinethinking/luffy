use crate::config::CONFIG;
use std::process::{Child, Command};

pub struct ServiceManager {}

impl ServiceManager {
    pub async fn start_services(&self) -> Result<Vec<Child>, Box<dyn std::error::Error>> {
        let mut children = Vec::new();

        if CONFIG.services.gateway.enabled {
            children.push(self.start_service("gateway").await?);
        }

        if CONFIG.services.media.enabled {
            children.push(self.start_service("media").await?);
        }

        Ok(children)
    }

    async fn start_service(&self, service_name: &str) -> Result<Child, Box<dyn std::error::Error>> {
        let command = match service_name {
            "gateway" => &CONFIG.services.gateway.command,
            "media" => &CONFIG.services.media.command,
            _ => return Err(format!("Unknown service: {}", service_name).into()),
        };

        let child = Command::new("sh")
            .arg("-c")
            .arg(command)
            .env("RUST_LOG", &CONFIG.log_level)
            .spawn()?;

        Ok(child)
    }
}
