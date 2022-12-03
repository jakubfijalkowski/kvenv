use std::collections::HashMap;

use clap::{arg, command, ArgGroup, Args};
use futures::TryFutureExt;
use reqwest::{self, StatusCode};
use serde::Deserialize;
use thiserror::Error;

use super::{convert::as_valid_env_name, Vault, VaultConfig};

#[derive(Args, Debug)]
#[command(group = ArgGroup::new("hashicorp"))]
pub struct HashicorpVaultConfig {
    /// Use Hashicorp Vault.
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
    HttpError(#[from] reqwest::Error),

    #[error("the Vault returned non-200 error code")]
    HttpStatusCodeError(StatusCode),

    #[error("cannot deserialize the response")]
    DeserializeError(#[source] reqwest::Error),

    #[error("the keys in the secret are not valid env names")]
    InvalidEnv(#[source] anyhow::Error),
}

pub struct HashicorpVault {
    address: String,
    token: String,
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
        })
    }
}

impl Vault for HashicorpVault {
    fn download_prefixed(&self, prefix: &str) -> anyhow::Result<Vec<(String, String)>> {
        todo!()
    }

    #[tokio::main]
    async fn download_json(&self, secret_name: &str) -> anyhow::Result<Vec<(String, String)>> {
        let client = reqwest::Client::new();
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
