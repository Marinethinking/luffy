use anyhow::{Context, Result};
use aws_config::{meta::region::RegionProviderChain, BehaviorVersion, Region};
use aws_sdk_lambda::{config::Credentials, primitives::Blob, Client as LambdaClient};
use mac_address::get_mac_address;

use crate::util;
use crate::vehicle::Vehicle;
use rumqttc::Client as MqttClient;

use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use tracing::error;

use tokio::sync::OnceCell;
use tracing::info;

use std::sync::{Arc, RwLock};
use tokio::sync::Mutex;

static AWS_CLIENT: OnceCell<AwsClient> = OnceCell::const_new();

pub struct AwsClient {
    vehicle: &'static Vehicle,
    mqtt_client: Arc<Mutex<Option<rumqttc::AsyncClient>>>,
    lambda_client: LambdaClient,
}

#[derive(Debug, Deserialize)]
pub struct IotCredentials {
    certificateArn: String,
    certificatePem: String,
    privateKey: String,
}

impl AwsClient {
    pub async fn get_aws_config() -> Result<aws_config::SdkConfig> {
        let region = env::var("AWS_REGION").unwrap_or_else(|_| "ca-central-1".into());
        let access_key = env::var("AWS_ACCESS_KEY_ID").context("AWS_ACCESS_KEY_ID not found")?;
        let secret_key =
            env::var("AWS_SECRET_ACCESS_KEY").context("AWS_SECRET_ACCESS_KEY not found")?;

        let credentials = Credentials::new(&access_key, &secret_key, None, None, "default");

        let config = aws_config::defaults(BehaviorVersion::latest())
            .region(RegionProviderChain::first_try(Region::new(region)))
            .credentials_provider(credentials)
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
                    vehicle: Vehicle::instance().await,
                    mqtt_client: Arc::new(Mutex::new(None)),
                    lambda_client: LambdaClient::new(&config),
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

        let lambda_name = env::var("LAMBDA_REGISTER").context("LAMBDA_REGISTER not found")?;
        let response = self.invoke_lambda(lambda_name, payload.to_string()).await?;

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
        let config_dir = dirs::config_dir()
            .context("Failed to get config directory")?
            .join("luffy");

        fs::create_dir_all(&config_dir)?;

        // Save certificate and private key to files
        fs::write(
            config_dir.join("certificate.pem"),
            &credentials.certificatePem,
        )?;
        fs::write(config_dir.join("private.key"), &credentials.privateKey)?;
        fs::write(
            config_dir.join("certificate.arn"),
            &credentials.certificateArn,
        )?;

        Ok(())
    }

    
}
