use anyhow::Result;
use config;
use once_cell::sync::Lazy;
use serde::Deserialize;
use std::fs;

pub static CONFIG: Lazy<Config> =
    Lazy::new(|| Config::load().expect("Failed to load configuration"));

#[derive(Debug, Deserialize)]
pub struct Config {
    pub aws: AwsConfig,
    // pub rumqttd: RumqttdConfig,
    // Add other top-level configs as needed
}

#[derive(Debug, Deserialize)]
pub struct RumqttdConfig {
    pub server: ServerConfig,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub listen: String,
    pub port: u16,
    pub max_connections: u32,
    pub max_client_id_len: u32,
}

#[derive(Debug, Deserialize)]
pub struct AwsConfig {
    pub region: String,
    pub iot: IotConfig,
    pub lambda: LambdaConfig,
}

#[derive(Debug, Deserialize)]
pub struct IotConfig {
    pub endpoint: String,
    pub port: u16,
    pub root_ca_path: String,
}

#[derive(Debug, Deserialize)]
pub struct LambdaConfig {
    pub register: String,
}

// ... other config structs ...

impl Config {
    pub fn load() -> Result<Self> {
        let env = std::env::var("RUST_ENV").unwrap_or_else(|_| "dev".to_string());
        let config_path = format!("config/{}.toml", env);

        let settings = config::Config::builder()
            .add_source(config::File::with_name(&config_path))
            .build()?;

        let config = settings.try_deserialize()?;
        Ok(config)
    }
}
