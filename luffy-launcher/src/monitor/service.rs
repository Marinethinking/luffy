pub struct ServiceState {
    pub name: String,
    pub status: String,
    pub last_health_report: std::time::SystemTime,
}
