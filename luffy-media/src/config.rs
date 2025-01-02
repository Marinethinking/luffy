use std::sync::LazyLock;

use luffy_common::config::{BaseConfig, LoadConfig};

use serde::Deserialize;

pub static CONFIG: LazyLock<MediaConfig> =
    LazyLock::new(|| MediaConfig::load_config("media").expect("Failed to load configuration"));

#[derive(Debug, Deserialize)]
pub struct MediaConfig {
    #[serde(flatten)]
    pub base: BaseConfig,
    pub log_level: String,
    pub cameras: Vec<CameraConfig>,
    pub websocket_port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CameraConfig {
    pub id: String,
    pub name: String,
    pub url: String,
    pub username: String,
    pub password: String,
}

impl LoadConfig for MediaConfig {}
