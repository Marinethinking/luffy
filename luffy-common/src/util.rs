use crate::config::BaseConfig;
use network_interface::{NetworkInterface, NetworkInterfaceConfig};
use uuid::Uuid;

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

use glob::Pattern;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
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

fn setup_dev_logging(log_level: &str) {
    let console_layer = tracing_subscriber::fmt::layer()
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_target(true)
        .with_file(true)
        .with_line_number(true)
        .pretty();

    tracing_subscriber::registry()
        .with(console_layer)
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

fn setup_prod_logging(log_level: &str, service_name: &str) -> bool {
    let log_dir = "/var/log/luffy";
    if std::fs::create_dir_all(log_dir).is_err() {
        return false;
    }

    let console_layer = tracing_subscriber::fmt::layer()
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_target(true)
        .with_file(true)
        .with_line_number(true)
        .pretty();

    // let all_log_appender = RollingFileAppender::new(
    //     Rotation::DAILY,
    //     log_dir,
    //     format!("{}-all.log", service_name),
    // );

    // let error_log_appender = RollingFileAppender::new(
    //     Rotation::DAILY,
    //     log_dir,
    //     format!("{}-error.log", service_name),
    // );

    let all_log_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(format!("{}-all", service_name)) // base name
        .filename_suffix("log") // extension
        .max_log_files(30)
        .build(log_dir)
        .unwrap();

    let error_log_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(format!("{}-error", service_name))
        .filename_suffix("log")
        .max_log_files(30)
        .build(log_dir)
        .unwrap();

    tracing_subscriber::registry()
        .with(console_layer)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(all_log_appender)
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_target(true)
                .with_file(true)
                .with_line_number(true),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(error_log_appender)
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_target(true)
                .with_file(true)
                .with_line_number(true)
                .with_filter(EnvFilter::new("error")),
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

    true
}

pub fn setup_logging(log_level: &str, service_name: &str) {
    let is_dev = std::env::var("RUST_ENV")
        .unwrap_or("test".to_string())
        .to_lowercase()
        == "dev";

    if is_dev || !setup_prod_logging(log_level, service_name) {
        setup_dev_logging(log_level)
    }
}

pub fn is_dev() -> bool {
    std::env::var("RUST_ENV")
        .unwrap_or("test".to_string())
        .to_lowercase()
        == "dev"
}

pub fn glob_match(pattern: &str, topic: &str) -> bool {
    let glob_pattern = pattern.replace('+', "*").replace('#', "**");

    let pattern = Pattern::new(&glob_pattern).unwrap();
    pattern.matches(topic)
}
