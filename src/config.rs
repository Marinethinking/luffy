use crate::ota::version::UpgradeStrategy;
use anyhow::Result;
use config;
use once_cell::sync::Lazy;
use serde::Deserialize;

pub static CONFIG: Lazy<Config> =
    Lazy::new(|| Config::load().expect("Failed to load configuration"));

#[derive(Debug, Deserialize)]
pub struct Config {
    pub feature: FeatureConfig,
    pub general: GeneralConfig,
    pub aws: AwsConfig,
    pub broker: BrokerConfig,
    pub mavlink: MavlinkConfig,
    pub iot: IotConfig,
    pub web: WebConfig,
    pub ota: OtaConfig,
}

#[derive(Debug, Deserialize)]
pub struct FeatureConfig {
    pub local_iot: bool,
    pub remote_iot: bool,
    pub broker: bool,
    pub mavlink: bool,
    pub ota: bool,
}

#[derive(Debug, Deserialize)]
pub struct GeneralConfig {
    pub log_level: String,
    pub vehicle_id: String,
}

#[derive(Debug, Deserialize)]
pub struct BrokerConfig {
    pub host: String,
    pub port: u16,
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
    pub local_interval: u64,
    pub remote_interval: u64,
}

#[derive(Debug, Deserialize)]
pub struct MavlinkConfig {
    pub connection_string: String,
}

#[derive(Debug, Deserialize)]
pub struct WebConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize)]
pub struct OtaConfig {
    pub strategy: UpgradeStrategy,
    pub check_interval: u64,
    pub version_check_url: String,
    pub image_name: String,
}

#[derive(Debug, Deserialize)]
pub struct SubscriptionConfig {
    pub enabled: bool,
}

// ... other config structs ...

impl Config {
    pub fn load() -> Result<Self> {
        let env = std::env::var("RUST_ENV").unwrap_or_else(|_| "dev".to_string());
        let config_path = format!("config/{}.toml", env);
        let fallback_path = format!("/etc/luffy/{}.toml", env);

        let config_builder = config::Config::builder();
        let config_builder = if std::path::Path::new(&config_path).exists() {
            config_builder.add_source(config::File::with_name(&config_path))
        } else {
            config_builder.add_source(config::File::with_name(&fallback_path))
        };

        let settings = config_builder.build()?;
        let config = settings.try_deserialize()?;
        Ok(config)
    }
}
