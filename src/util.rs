use mac_address::get_mac_address;

pub fn get_device_mac() -> String {
    get_mac_address()
        .ok()
        .flatten()
        .map(|addr| {
            addr.to_string()
                .chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<String>()
                .to_uppercase()
        })
        .unwrap_or_else(|| "unknown".to_string())
}
