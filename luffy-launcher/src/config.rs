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
}

#[derive(Debug, Deserialize)]
pub struct WebConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct OtaConfig {
    pub strategy: String,
    pub check_interval: u32,
    pub github_repo: String,
    pub gateway: bool,
    pub media: bool,
    pub download_dir: Option<String>,
}

impl LoadConfig for LauncherConfig {}

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
