use anyhow::{Context, Result};
use aws_config::{meta::region::RegionProviderChain, BehaviorVersion, Region};
use aws_sdk_lambda::{config::Credentials, primitives::Blob, Client as LambdaClient};
use aws_sdk_s3::Client as S3Client;

use crate::config::CONFIG;
use crate::util;

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
        let device_id = util::get_device_mac();

        let payload = serde_json::json!({
            "typeName": "Query",
            "fieldName": "registerIotThing",
            "arguments": {
                "thingName": device_id,
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
        info!("Credentials saved successfully");
        Ok(())
    }

    pub async fn upload_to_s3(&self, data: Vec<u8>, key: &str) -> Result<()> {
        self.s3_client
            .put_object()
            .bucket(&CONFIG.ota.s3_bucket)
            .key(key)
            .body(data.into())
            .send()
            .await?;
        
        Ok(())
    }

    pub async fn download_from_s3(&self, key: &str) -> Result<Vec<u8>> {
        let response = self.s3_client
            .get_object()
            .bucket(&CONFIG.ota.s3_bucket)
            .key(key)
            .send()
            .await?;
            
        Ok(response.body.collect().await?.into_bytes().to_vec())
    }
}
