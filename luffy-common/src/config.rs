use anyhow::Result;
use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct BaseConfig {
    pub vehicle_id: String,
}

pub trait LoadConfig {
    fn load_config(service_name: &str) -> Result<Self, ConfigError>
    where
        Self: Sized + serde::de::DeserializeOwned,
    {
        // Try development path first, relative to project root
        let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap_or(PathBuf::from(".").as_path())
            .join("luffy-deploy/config/development");
        let prod_path = PathBuf::from("/etc/luffy");

        let config_dir = if dev_path.join(format!("{}.toml", service_name)).exists() {
            dev_path
        } else if prod_path.join(format!("{}.toml", service_name)).exists() {
            prod_path
        } else {
            return Err(ConfigError::NotFound(format!(
                "Config file not found in {:?}",
                prod_path.join(format!("{}.toml", service_name))
            )));
        };

        let config = Config::builder()
            // Base config first (if it exists)
            .add_source(File::from(config_dir.join("base.toml")).required(false))
            // Service-specific config (required)
            .add_source(File::from(
                config_dir.join(format!("{}.toml", service_name)),
            ))
            // Environment variables override
            .add_source(Environment::with_prefix("LUFFY"))
            .build()?;

        config.try_deserialize()
    }
}
