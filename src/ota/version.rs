use anyhow::{Result, anyhow};
use semver::Version;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use crate::config::CONFIG;

#[derive(Debug, Serialize, Deserialize)]
pub struct VehicleInfo {
    pub id: String,
    pub subscription: SubscriptionType,
    pub boat_mode: BoatMode,
    pub current_version: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum SubscriptionType {
    Basic,
    Premium,
    Enterprise,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum BoatMode {
    Manual,
    Autonomous,
    Training,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReleaseInfo {
    pub version: String,
    pub required_subscription: SubscriptionType,
    pub changelog: String,
    pub release_date: String,
    pub minimum_required_version: Option<String>,
}

pub struct VersionManager {
    vehicle_info: VehicleInfo,
    current_version: Version,
}

impl VersionManager {
    pub fn new(vehicle_info: VehicleInfo) -> Result<Self> {
        let current_version = Version::parse(&vehicle_info.current_version)
            .map_err(|e| anyhow!("Invalid version format: {}", e))?;

        Ok(Self {
            vehicle_info,
            current_version,
        })
    }

    pub async fn check_update_availability(&self) -> Result<Option<ReleaseInfo>> {
        // TODO: Implement AWS IoT Shadow subscription for version updates
        // The vehicle should subscribe to a shadow topic like:
        // $aws/things/{vehicle_id}/shadow/name/version/update
        
        // For now, we'll just check the version endpoint
        let release_info = self.fetch_latest_release_info().await?;
        
        if self.is_update_applicable(&release_info)? {
            Ok(Some(release_info))
        } else {
            Ok(None)
        }
    }

    async fn fetch_latest_release_info(&self) -> Result<ReleaseInfo> {
        let client = reqwest::Client::new();
        let release_info = client
            .get(&CONFIG.ota.version_check_url)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?
            .json::<ReleaseInfo>()
            .await?;

        Ok(release_info)
    }

    fn is_update_applicable(&self, release_info: &ReleaseInfo) -> Result<bool> {
        let release_version = Version::parse(&release_info.version)?;
        
        // Check if current version is lower than release version
        if release_version <= self.current_version {
            return Ok(false);
        }

        // Check subscription level requirement
        match (&self.vehicle_info.subscription, &release_info.required_subscription) {
            (SubscriptionType::Enterprise, _) => (),
            (SubscriptionType::Premium, SubscriptionType::Basic | SubscriptionType::Premium) => (),
            (SubscriptionType::Basic, SubscriptionType::Basic) => (),
            _ => {
                warn!("Update requires higher subscription level");
                return Ok(false);
            }
        }

        // Check minimum required version if specified
        if let Some(min_version) = &release_info.minimum_required_version {
            let min_version = Version::parse(min_version)?;
            if self.current_version < min_version {
                warn!("Current version is below minimum required version");
                return Ok(false);
            }
        }

        Ok(true)
    }

    pub fn get_current_version(&self) -> &Version {
        &self.current_version
    }

    pub fn get_vehicle_info(&self) -> &VehicleInfo {
        &self.vehicle_info
    }
} 