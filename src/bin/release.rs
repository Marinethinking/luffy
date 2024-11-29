use anyhow::{anyhow, Result};

use luffy::config::CONFIG;

use chrono::Utc;
use luffy::aws_client::AwsClient;
use serde::Serialize;
use std::process::Command;
use tracing::info;

#[derive(Serialize)]
struct ReleaseInfo {
    version: String,
    required_subscription: String,
    changelog: String,
    release_date: String,
    minimum_required_version: String,
}

async fn upload_to_s3(client: &AwsClient, data: Vec<u8>, key: &str) -> Result<()> {
    client.upload_to_s3(data, key).await
}

#[tokio::main]
async fn main() -> Result<()> {
    let version = env!("CARGO_PKG_VERSION");
    let target = "aarch64-unknown-linux-gnu";
    info!("🚀 Building release version {version} for {target}...");

    // Build release
    let status = Command::new("cross")
        .args(["build", "--release", "--target", target])
        .status()?;

    if !status.success() {
        return Err(anyhow!("Build failed"));
    }

    // Get AWS client instance
    let client = AwsClient::instance().await;

    info!("☁️  Uploading to S3...");

    // Read binary
    let binary_path = format!("target/{}/release/luffy", target);
    let binary = std::fs::read(&binary_path)?;

    // Extract just the architecture part from the target triple
    let arch = target.split('-').next().unwrap_or("unknown");

    // Use the shortened architecture name in file paths
    let versioned_key = format!("{}/luffy-{}-{}", CONFIG.ota.release_path, version, arch);
    upload_to_s3(&client, binary.clone(), &versioned_key).await?;

    // Update latest key as well
    let latest_key = format!("{}/luffy-latest-{}", CONFIG.ota.release_path, arch);
    upload_to_s3(&client, binary, &latest_key).await?;

    // Create and upload release info
    let release_info = ReleaseInfo {
        version: version.to_string(),
        required_subscription: "Basic".to_string(),
        changelog: format!("New release {}", version),
        release_date: Utc::now().date_naive().to_string(),
        minimum_required_version: "0.1.0".to_string(),
    };
    let release_info_json = serde_json::to_vec(&release_info)?;
    let release_info_key = format!("{}/release-info-{}.json", CONFIG.ota.release_path, arch);
    upload_to_s3(&client, release_info_json, &release_info_key).await?;

    println!("✅ Release {version} for {target} uploaded successfully!");
    println!("Files uploaded:");
    println!("- s3://{}/{}", CONFIG.ota.s3_bucket, versioned_key);
    println!("- s3://{}/{}", CONFIG.ota.s3_bucket, latest_key);
    println!("- s3://{}/{}", CONFIG.ota.s3_bucket, release_info_key);

    Ok(())
}
