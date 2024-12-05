use crate::config::BaseConfig;
use network_interface::{NetworkInterface, NetworkInterfaceConfig};
use uuid::Uuid;

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
