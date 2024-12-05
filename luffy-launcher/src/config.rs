use luffy_common::config::{BaseConfig, LoadConfig};
use once_cell::sync::Lazy;
use serde::Deserialize;

pub static CONFIG: Lazy<LauncherConfig> =
    Lazy::new(|| LauncherConfig::load_config("launcher").expect("Failed to load configuration"));

#[derive(Debug, Deserialize)]
pub struct LauncherConfig {
    #[serde(flatten)]
    pub base: BaseConfig,
    pub log_level: String,
    pub web: WebConfig,
    pub ota: OtaConfig,
    pub services: ServicesConfig,
}

#[derive(Debug, Deserialize)]
pub struct WebConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize)]
pub struct OtaConfig {
    pub strategy: String,
    pub check_interval: u32,
    pub version_check_url: String,
    pub image_name: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServicesConfig {
    pub gateway: ServiceGatewayConfig,
    pub media: ServiceMediaConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServiceGatewayConfig {
    pub enabled: bool,
    pub command: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServiceMediaConfig {
    pub enabled: bool,
    pub command: String,
}

impl LoadConfig for LauncherConfig {}
