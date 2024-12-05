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
        let config_dir =
            std::env::var("CONFIG_DIR").unwrap_or_else(|_| "luffy-deploy/config".to_string());

        let env = std::env::var("RUST_ENV").unwrap_or_else(|_| "development".to_string());

        let config = Config::builder()
            // Base config first
            .add_source(File::from(
                PathBuf::from(&config_dir).join(&env).join("base.toml"),
            ))
            // Service-specific config
            .add_source(File::from(
                PathBuf::from(&config_dir)
                    .join(&env)
                    .join(format!("{}.toml", service_name)),
            ))
            // Environment variables override
            .add_source(Environment::with_prefix("LUFFY"))
            .build()?;

        config.try_deserialize()
    }
}
