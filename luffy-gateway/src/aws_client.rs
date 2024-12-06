use anyhow::{Context, Result};
use aws_config::{meta::region::RegionProviderChain, BehaviorVersion, Region};
use aws_sdk_lambda::{primitives::Blob, Client as LambdaClient};
use aws_sdk_s3::Client as S3Client;
use luffy_common::util;

use crate::config::CONFIG;

use serde::Deserialize;

use std::fs;

use tokio::sync::OnceCell;
use tracing::info;

static AWS_CLIENT: OnceCell<AwsClient> = OnceCell::const_new();

pub struct AwsClient {
    lambda_client: LambdaClient,
    s3_client: S3Client,
}

#[derive(Debug, Deserialize)]
pub struct IotCredentials {
    #[serde(rename = "certificateArn")]
    certificate_arn: String,
    #[serde(rename = "certificatePem")]
    certificate_pem: String,
    #[serde(rename = "privateKey")]
    private_key: String,
}

impl AwsClient {
    pub async fn get_aws_config() -> Result<aws_config::SdkConfig> {
        let region = &CONFIG.aws.region;

        let config = aws_config::defaults(BehaviorVersion::latest())
            .region(RegionProviderChain::first_try(Region::new(region)))
            .load()
            .await;
        Ok(config)
    }

    pub async fn instance() -> &'static AwsClient {
        AWS_CLIENT
            .get_or_init(|| async {
                let config = Self::get_aws_config()
                    .await
                    .context("Failed to get AWS config")
                    .unwrap();

                AwsClient {
                    lambda_client: LambdaClient::new(&config),
                    s3_client: S3Client::new(&config),
                }
            })
            .await
    }

    pub async fn invoke_lambda(&self, function: String, payload: String) -> Result<Blob> {
        let response = self
            .lambda_client
            .invoke()
            .function_name(function)
            .payload(Blob::new(payload.as_bytes()))
            .send()
            .await?;
        response
            .payload()
            .context("Empty response from Lambda")
            .cloned()
    }

    pub async fn register_device(&self) -> Result<IotCredentials> {
        info!("Registering device...");
        let vehicle_id = util::get_vehicle_id(&CONFIG.base);

        let payload = serde_json::json!({
            "typeName": "Query",
            "fieldName": "registerIotThing",
            "arguments": {
                "thingName": vehicle_id,
                "thingType": "zoro"
            }
        });

        let lambda_name = &CONFIG.aws.lambda.register;
        let response = self
            .invoke_lambda(lambda_name.to_string(), payload.to_string())
            .await?;

        // Print raw response as string
        let raw_response = String::from_utf8_lossy(response.as_ref());
        info!("Raw Lambda response: {}", raw_response);

        let credentials: IotCredentials = serde_json::from_slice(response.as_ref())
            .context("Failed to deserialize Lambda response")?;

        // Save credentials locally
        self.save_credentials(&credentials)?;

        Ok(credentials)
    }

    fn save_credentials(&self, credentials: &IotCredentials) -> Result<()> {
        info!("Saving credentials...");

        match std::env::var("RUST_ENV").as_deref() {
            Ok("dev") => self.save_credentials_dev(credentials),
            _ => self.save_credentials_deb(credentials),
        }
    }

    fn save_credentials_dev(&self, credentials: &IotCredentials) -> Result<()> {
        let config_dir = dirs::config_dir()
            .context("Failed to get config directory")?
            .join("luffy");

        fs::create_dir_all(&config_dir)?;

        // Save certificate and private key to files
        fs::write(
            config_dir.join("certificate.pem"),
            &credentials.certificate_pem,
        )?;
        fs::write(config_dir.join("private.key"), &credentials.private_key)?;
        fs::write(
            config_dir.join("certificate.arn"),
            &credentials.certificate_arn,
        )?;
        info!("Credentials saved successfully in dev mode");
        Ok(())
    }

    fn save_credentials_deb(&self, credentials: &IotCredentials) -> Result<()> {
        let config_dir = std::path::PathBuf::from("/etc/luffy");

        // Create directory with proper permissions (readable by owner and group)
        if !config_dir.exists() {
            fs::create_dir_all(&config_dir).context("Failed to create config directory")?;
            // Set directory permissions to 755 (rwxr-xr-x)
            let mut perms = fs::metadata(&config_dir)?.permissions();
            std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
            fs::set_permissions(&config_dir, perms)?;
        }

        // Save certificate and private key to files with restricted permissions
        for (filename, content) in [
            ("certificate.pem", &credentials.certificate_pem),
            ("private.key", &credentials.private_key),
            ("certificate.arn", &credentials.certificate_arn),
        ] {
            let path = config_dir.join(filename);
            fs::write(&path, content).with_context(|| format!("Failed to write {}", filename))?;
            // Set file permissions to 640 (rw-r-----)
            let mut perms = fs::metadata(&path)?.permissions();
            std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o640);
            fs::set_permissions(&path, perms)?;
        }

        info!("Credentials saved successfully in deb mode");
        Ok(())
    }

    pub fn s3(&self) -> &aws_sdk_s3::Client {
        &self.s3_client
    }
}
