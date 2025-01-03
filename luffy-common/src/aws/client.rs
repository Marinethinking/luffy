use anyhow::{Context, Result};
use aws_config::{meta::region::RegionProviderChain, BehaviorVersion, Region};
use aws_sdk_lambda::{primitives::Blob, Client as LambdaClient};
use aws_sdk_s3::Client as S3Client;
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
    pub certificate_arn: String,
    #[serde(rename = "certificatePem")]
    pub certificate_pem: String,
    #[serde(rename = "privateKey")]
    pub private_key: String,
}

impl AwsClient {
    pub async fn get_aws_config() -> Result<aws_config::SdkConfig> {
        let region = Region::new("ca-central-1");

        let config = aws_config::defaults(BehaviorVersion::latest())
            .region(RegionProviderChain::first_try(region))
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
        let vehicle_id = "luffy-dev"; // TODO: Get from config

        let payload = serde_json::json!({
            "typeName": "Query",
            "fieldName": "registerIotThing",
            "arguments": {
                "thingName": vehicle_id,
                "thingType": "zoro"
            }
        });

        let lambda_name = "arn:aws:lambda:ca-central-1:583818069008:function:amplify-d34e88yymcb7ax-de-registerIotThinglambdaCE-j14AZkH1hKNp";
        let response = self
            .invoke_lambda(lambda_name.to_string(), payload.to_string())
            .await?;

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

        if !config_dir.exists() {
            fs::create_dir_all(&config_dir).context("Failed to create config directory")?;
            let mut perms = fs::metadata(&config_dir)?.permissions();
            std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
            fs::set_permissions(&config_dir, perms)?;
        }

        for (filename, content) in [
            ("certificate.pem", &credentials.certificate_pem),
            ("private.key", &credentials.private_key),
            ("certificate.arn", &credentials.certificate_arn),
        ] {
            let path = config_dir.join(filename);
            fs::write(&path, content).with_context(|| format!("Failed to write {}", filename))?;
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