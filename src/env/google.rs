use clap::Clap;
use googapis::{
    google::cloud::secretmanager::v1::{
        secret_manager_service_client::SecretManagerServiceClient, AccessSecretVersionRequest,
        GetSecretRequest, GetSecretVersionRequest, ListSecretsRequest,
    },
    CERTIFICATES,
};
use gouth::Token;
use serde_json::Value;
use std::path::PathBuf;
use thiserror::Error;
use tonic::{
    metadata::MetadataValue,
    transport::{Certificate, Channel, ClientTlsConfig},
    Request,
};

use super::{convert::decode_env_from_json, Vault, VaultConfig};

#[derive(Clap, Debug)]
pub struct GoogleConfig {
    /// Use Google Secret Manager.
    #[clap(name = "google", long = "google", requires = "project")]
    enabled: bool,

    /// [Google] The path to credentials file. Leave blank to use gouth default credentials resolution.
    #[clap(
        long,
        parse(from_os_str),
        env = "GOOGLE_APPLICATION_CREDENTIALS",
        display_order = 110
    )]
    credentials_file: Option<PathBuf>,

    /// [Google] Google project to use.
    #[clap(long, display_order = 111)]
    project: Option<String>,
}

#[derive(Error, Debug)]
pub enum GoogleError {
    #[error("Tonic configuration error")]
    TonicError(#[source] tonic::transport::Error),
    #[error("Google SA configuration is invalid")]
    ConfigurationError(#[source] gouth::Error),
    #[error("cannot load secret from Secret Manager")]
    SecretManagerError(#[source] tonic::Status),
    #[error("the secret is empty")]
    EmptySecret,
}

pub type Result<T, E = GoogleError> = std::result::Result<T, E>;

impl VaultConfig for GoogleConfig {
    type Vault = GoogleConfig;

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn into_vault(self) -> anyhow::Result<Self::Vault> {
        Ok(self)
    }
}

impl GoogleConfig {
    async fn to_client(&self) -> Result<SecretManagerServiceClient<Channel>> {
        let tls_config = ClientTlsConfig::new()
            .ca_certificate(Certificate::from_pem(CERTIFICATES))
            .domain_name("secretmanager.googleapis.com");

        let channel = Channel::from_static("https://secretmanager.googleapis.com")
            .tls_config(tls_config)
            .map_err(GoogleError::TonicError)?
            .connect()
            .await
            .map_err(GoogleError::TonicError)?;

        let token = self.to_token()?;

        let client = SecretManagerServiceClient::with_interceptor(
            channel,
            move |mut req: tonic::Request<()>| {
                let token = token
                    .header_value()
                    .map_err(|e| tonic::Status::unknown(e.to_string()))?;
                let meta = MetadataValue::from_str(&*token)
                    .map_err(|e| tonic::Status::unknown(e.to_string()))?;
                req.metadata_mut().insert("authorization", meta);
                Ok(req)
            },
        );

        Ok(client)
    }

    fn to_token(&self) -> Result<Token> {
        let token = if let Some(path) = &self.credentials_file {
            gouth::Builder::new().file(path).build()
        } else {
            Token::new()
        };
        Ok(token.map_err(GoogleError::ConfigurationError)?)
    }
}

impl Vault for GoogleConfig {
    #[tokio::main]
    async fn download_prefixed(&self, prefix: &str) -> anyhow::Result<Vec<(String, String)>> {
        todo!()
    }

    #[tokio::main]
    async fn download_json(&self, secret_name: &str) -> anyhow::Result<Vec<(String, String)>> {
        let mut client = self.to_client().await?;
        let response = client
            .access_secret_version(Request::new(AccessSecretVersionRequest {
                name: format!(
                    "projects/{}/secrets/{}/versions/latest",
                    self.project.as_ref().unwrap(),
                    secret_name
                ),
            }))
            .await
            .map_err(GoogleError::SecretManagerError)?;
        let payload = response
            .get_ref()
            .payload
            .as_ref()
            .ok_or(GoogleError::EmptySecret)?;
        let value: Value = serde_json::from_slice(&payload.data)?;
        decode_env_from_json(secret_name, value)
    }
}
