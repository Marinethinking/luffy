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
    pub rumqttd: RumqttdConfig,
    pub mavlink: MavlinkConfig,
    pub iot: IotConfig,
    pub web: WebConfig,
}

#[derive(Debug, Deserialize)]
pub struct RumqttdConfig {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
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
    pub iot: AwsIotConfig,
    pub lambda: LambdaConfig,
}

#[derive(Debug, Deserialize)]
pub struct AwsIotConfig {
    pub endpoint: String,
    pub port: u16,
    pub root_ca_path: String,
}

#[derive(Debug, Deserialize)]
pub struct LambdaConfig {
    pub register: String,
}

#[derive(Debug, Deserialize)]
pub struct IotConfig {
    pub enabled: bool,
    pub telemetry: IotTelemetryConfig,
}

#[derive(Debug, Deserialize)]
pub struct IotTelemetryConfig {
    pub local_interval: u64,
    pub remote_interval: u64,
}

#[derive(Debug, Deserialize)]
pub struct MavlinkConfig {
    pub connection_string: String,
}

#[derive(Debug, Deserialize)]
pub struct WebConfig {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
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
