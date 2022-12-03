use std::{collections::HashMap, path::PathBuf};

use clap::{arg, command, ArgGroup, Args};
use reqwest::{self, StatusCode};
use serde::Deserialize;
use thiserror::Error;
use tokio::io::AsyncReadExt;

use super::{convert::as_valid_env_name, Vault, VaultConfig};

#[derive(Args, Debug)]
#[command(group = ArgGroup::new("hashicorp"))]
pub struct HashicorpVaultConfig {
    /// Use Hashicorp Vault.
    /// Vault mode works differently to other clouds. When "single secret" mode is selected, it
    /// interprets the document as a key-value document, where key is the environment variable
    /// name.
    /// When in perfixed mode, it uses the name of matched secret as a environment variable name.
    #[arg(
        name = "vault",
        long = "vault",
        group = "cloud",
        requires = "vault",
        display_order = 400
    )]
    enabled: bool,

    /// [Hashicorp Vault] Address of the vault.
    #[arg(long, env = "VAULT_ADDR", display_order = 401)]
    vault_address: Option<String>,

    /// [Hashicorp Vault] Token that should be used to authorize the request.
    #[arg(long, env = "VAULT_TOKEN", hide_env_values = true, display_order = 402)]
    vault_token: Option<String>,

    #[arg(long, value_parser, env = "VAULT_CACERT", display_order = 403)]
    vault_cacert: Option<PathBuf>,
}

#[derive(Error, Debug)]
pub enum HashicorpVaultError {
    #[error("secret '{0}' does not exist")]
    SecretNotFound(String),

    #[error("the vault token is invalid")]
    UnauthorizedError,

    #[error("the token does not have access to secret '{0}'")]
    ForbiddenError(String),

    #[error("HTTP error occurred")]
    HttpError(#[source] reqwest::Error),

    #[error("the Vault returned non-200 error code")]
    HttpStatusCodeError(StatusCode),

    #[error("cannot deserialize the response")]
    DeserializeError(#[source] reqwest::Error),

    #[error("the keys in the secret are not valid env names")]
    InvalidEnv(#[source] anyhow::Error),

    #[error("the configuration is invalid")]
    ConfigurationError(#[from] anyhow::Error),
}

pub struct HashicorpVault {
    address: String,
    token: String,
    cacert: Option<PathBuf>,
}

impl VaultConfig for HashicorpVaultConfig {
    type Vault = HashicorpVault;

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn into_vault(self) -> anyhow::Result<Self::Vault> {
        Ok(Self::Vault {
            address: self.vault_address.unwrap(),
            token: self.vault_token.unwrap(),
            cacert: self.vault_cacert,
        })
    }
}

impl HashicorpVault {
    async fn client(&self) -> Result<reqwest::Client, HashicorpVaultError> {
        let mut builder = reqwest::Client::builder().user_agent("kvenv");

        if let Some(path) = self.cacert.as_ref() {
            let mut buffer = Vec::new();
            {
                let mut file = tokio::fs::File::open(path)
                    .await
                    .map_err(anyhow::Error::new)?;
                file.read_to_end(&mut buffer)
                    .await
                    .map_err(anyhow::Error::new)?;
            }
            let cert = reqwest::Certificate::from_pem(&buffer).map_err(anyhow::Error::new)?;
            builder = builder.add_root_certificate(cert);
        }

        builder
            .build()
            .map_err(anyhow::Error::new)
            .map_err(HashicorpVaultError::ConfigurationError)
    }
}

impl Vault for HashicorpVault {
    fn download_prefixed(&self, prefix: &str) -> anyhow::Result<Vec<(String, String)>> {
        todo!()
    }

    #[tokio::main]
    async fn download_json(&self, secret_name: &str) -> anyhow::Result<Vec<(String, String)>> {
        let client = self.client().await?;
        let response = client
            .get(format!("{}/v1/secret/data/{}", self.address, secret_name))
            .header("X-Vault-Token", &self.token)
            .send()
            .await
            .map_err(HashicorpVaultError::HttpError)?;
        handle_common_errors(secret_name, &response)?;

        let data: SecretResponse = response
            .json()
            .await
            .map_err(HashicorpVaultError::DeserializeError)?;
        let result = data
            .data
            .data
            .into_iter()
            .map(|(k, v)| as_valid_env_name(k).map(|k| (k, v)))
            .collect::<anyhow::Result<Vec<_>>>()
            .map_err(HashicorpVaultError::InvalidEnv)?;
        Ok(result)
    }
}

fn handle_common_errors(
    secret_name: &str,
    response: &reqwest::Response,
) -> anyhow::Result<(), HashicorpVaultError> {
    match response.status() {
        StatusCode::NOT_FOUND => Err(HashicorpVaultError::SecretNotFound(secret_name.to_string())),
        StatusCode::UNAUTHORIZED => Err(HashicorpVaultError::UnauthorizedError),
        StatusCode::FORBIDDEN => Err(HashicorpVaultError::ForbiddenError(secret_name.to_string())),
        StatusCode::OK => Ok(()),
        other => Err(HashicorpVaultError::HttpStatusCodeError(other)),
    }
}

#[derive(Deserialize, Debug)]
struct SecretResponse {
    pub data: Secret,
}

#[derive(Deserialize, Debug)]
struct Secret {
    pub data: HashMap<String, String>,
}
