use crate::config::BaseConfig;
use network_interface::{NetworkInterface, NetworkInterfaceConfig};
use uuid::Uuid;

use tracing::{error, info};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

pub fn get_vehicle_id(config: &BaseConfig) -> String {
    std::env::var("VEHICLE_ID").unwrap_or_else(|_| config.vehicle_id.clone())
}

pub fn get_mac_address() -> String {
    let preferred_interfaces = ["eth0", "en0", "wlan0", "enp0s3"];

    if let Ok(interfaces) = NetworkInterface::show() {
        for preferred_name in preferred_interfaces {
            if let Some(interface) = interfaces.iter().find(|iface| iface.name == preferred_name) {
                if let Some(mac) = &interface.mac_addr {
                    return mac
                        .to_string()
                        .chars()
                        .filter(|c| c.is_alphanumeric())
                        .collect::<String>()
                        .to_uppercase();
                }
            }
        }
    }

    Uuid::new_v4().to_string()
}

pub fn setup_logging(log_level: &str) {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_target(true)
                .with_file(true)
                .with_line_number(true)
                .pretty(),
        )
        .with(
            EnvFilter::from_default_env()
                .add_directive(log_level.parse().unwrap())
                .add_directive("tokio=debug".parse().unwrap())
                .add_directive("runtime=debug".parse().unwrap())
                .add_directive("rumqttc=info".parse().unwrap())
                .add_directive("rumqttd=info".parse().unwrap()),
        )
        .try_init()
        .expect("Failed to initialize logging");
}

pub fn is_dev() -> bool {
    std::env::var("RUST_ENV")
        .unwrap_or("test".to_string())
        .to_lowercase()
        == "dev"
}
