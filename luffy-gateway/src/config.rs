use luffy_common::config::{BaseConfig, LoadConfig};
use once_cell::sync::Lazy;
use serde::Deserialize;

pub static CONFIG: Lazy<GatewayConfig> =
    Lazy::new(|| GatewayConfig::load_config("gateway").expect("Failed to load configuration"));

#[derive(Debug, Deserialize)]
pub struct GatewayConfig {
    #[serde(flatten)]
    pub base: BaseConfig,
    pub log_level: String,
    pub feature: FeatureConfig,

    pub aws: AwsConfig,
    pub broker: BrokerConfig,
    pub mavlink: MavlinkConfig,
    pub iot: IotConfig,
}

#[derive(Debug, Deserialize)]
pub struct FeatureConfig {
    pub local_iot: bool,
    pub remote_iot: bool,
    pub broker: bool,
    pub mavlink: bool,
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

impl LoadConfig for GatewayConfig {}
