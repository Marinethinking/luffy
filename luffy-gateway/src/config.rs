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
    pub mavlink: MavlinkConfig,
    pub iot: IotConfig,
    pub ota: OtaConfig,
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

#[derive(Debug, Deserialize, Clone)]
pub struct OtaConfig {
    pub enable: bool,
    pub strategy: String,
    pub check_interval: u32,
    pub download_dir: Option<String>,
    pub github_repo: String,
    pub launcher: bool,
}

impl LoadConfig for GatewayConfig {}

impl From<OtaConfig> for luffy_common::ota::version::VersionConfig {
    fn from(config: OtaConfig) -> Self {
        Self {
            strategy: config.strategy,
            check_interval: config.check_interval,
            download_dir: config.download_dir,
            github_repo: config.github_repo,
        }
    }
}
