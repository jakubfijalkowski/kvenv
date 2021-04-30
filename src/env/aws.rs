use clap::{ArgSettings, Clap};
use futures::future::try_join_all;
use rusoto_core::{request::TlsError, HttpClient, Region};
use rusoto_credential::{CredentialsError, DefaultCredentialsProvider, StaticProvider};
use rusoto_secretsmanager::{
    GetSecretValueError, GetSecretValueRequest, GetSecretValueResponse, ListSecretsError,
    ListSecretsRequest, SecretsManager, SecretsManagerClient,
};
use serde_json::Value;
use thiserror::Error;

use super::{convert::decode_env_from_json, Vault, VaultConfig};

#[derive(Clap, Debug)]
pub struct AwsConfig {
    /// Use AWS Secrets Manager.
    #[clap(name = "aws", long = "aws", group = "cloud", requires = "aws-region")]
    enabled: bool,

    /// [AWS] The Access Key Id. Requires `secret_access_key` if provided. If not specified,
    /// default rusoto credential matching is used.
    #[clap(
        long,
        env = "AWS_ACCESS_KEY_ID",
        display_order = 120,
        requires = "aws-secret-access-key"
    )]
    aws_access_key_id: Option<String>,

    /// [AWS] The Secret Access Key. Requires `access_key_id` if provided. If not specified,
    /// default rusoto credential matching is used.
    #[clap(
        long,
        env = "AWS_SECRET_ACCESS_KEY",
        setting = ArgSettings::HideEnvValues,
        display_order = 121,
    )]
    aws_secret_access_key: Option<String>,

    /// [AWS] AWS region.
    #[clap(long, env = "AWS_REGION", display_order = 122)]
    aws_region: Option<Region>,
}

#[derive(Error, Debug)]
pub enum AwsError {
    #[error("rusoto HttpClient error")]
    TlsError(#[source] TlsError),
    #[error("rusoto HttpClient error")]
    CredentialsError(#[source] CredentialsError),
    #[error("cannot load secret from Secrets Manager")]
    GetSecretError(#[source] rusoto_core::RusotoError<GetSecretValueError>),
    #[error("cannot list secrets from Secrets Manager")]
    ListSecretsError(#[source] rusoto_core::RusotoError<ListSecretsError>),
    #[error("cannot decode secret")]
    DecodeError(#[source] serde_json::Error),
    #[error("there are no secrets in the Secrets Manager")]
    NoSecrets,
}

pub type Result<T, E = AwsError> = std::result::Result<T, E>;

pub struct AwsVault {
    client: SecretsManagerClient,
}

impl VaultConfig for AwsConfig {
    type Vault = AwsVault;

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn into_vault(self) -> anyhow::Result<Self::Vault> {
        let http_client = HttpClient::new().map_err(AwsError::TlsError)?;
        if let Some(key_id) = self.aws_access_key_id {
            let secret = self.aws_secret_access_key.unwrap();
            let provider = StaticProvider::new_minimal(key_id, secret);
            Ok(Self::Vault {
                client: SecretsManagerClient::new_with(
                    http_client,
                    provider,
                    self.aws_region.unwrap(),
                ),
            })
        } else {
            let provider = DefaultCredentialsProvider::new().map_err(AwsError::CredentialsError)?;
            Ok(Self::Vault {
                client: SecretsManagerClient::new_with(
                    http_client,
                    provider,
                    self.aws_region.unwrap(),
                ),
            })
        }
    }
}

impl Vault for AwsVault {
    #[tokio::main]
    async fn download_prefixed(&self, prefix: &str) -> anyhow::Result<Vec<(String, String)>> {
        let list = self
            .client
            .list_secrets(ListSecretsRequest {
                max_results: Some(100),
                ..Default::default()
            })
            .await
            .map_err(AwsError::ListSecretsError)?;
        let secrets: Vec<_> = list
            .secret_list
            .ok_or(AwsError::NoSecrets)?
            .into_iter()
            .filter(|x| {
                x.name
                    .as_ref()
                    .map(|n| n.starts_with(prefix))
                    .unwrap_or(false)
            })
            .collect();
        let results = secrets.into_iter().map(|s| async {
            let name = s.name.unwrap();
            let secret = self
                .client
                .get_secret_value(GetSecretValueRequest {
                    secret_id: name.clone(),
                    version_id: None,
                    version_stage: None,
                })
                .await
                .map_err(AwsError::GetSecretError)?;
            let value = decode_secret(secret)?;
            decode_env_from_json(&name, value)
        });
        let values: Vec<_> = try_join_all(results).await?.into_iter().flatten().collect();
        Ok(values)
    }

    #[tokio::main]
    async fn download_json(&self, secret_name: &str) -> anyhow::Result<Vec<(String, String)>> {
        let secret = self
            .client
            .get_secret_value(GetSecretValueRequest {
                secret_id: secret_name.to_string(),
                version_id: None,
                version_stage: None,
            })
            .await
            .map_err(AwsError::GetSecretError)?;
        let value = decode_secret(secret)?;
        decode_env_from_json(secret_name, value)
    }
}

fn decode_secret(secret: GetSecretValueResponse) -> Result<Value> {
    secret
        .secret_string
        .as_ref()
        .map(|x| serde_json::from_str(&x[..]))
        .or_else(|| secret.secret_binary.map(|b| serde_json::from_slice(&b)))
        .unwrap()
        .map_err(AwsError::DecodeError)
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "integration-tests")]
    use super::*;

    #[cfg(feature = "integration-tests")]
    macro_rules! env {
        ($a:expr) => {
            $a.to_string()
        };
        ($a:expr, $b:expr) => {
            ($a.to_string(), $b.to_string())
        };
    }

    #[cfg(feature = "integration-tests")]
    #[test]
    fn integration_tests_single_value() {
        use std::env::var as env_var;
        let cfg = AwsConfig {
            enabled: true,
            aws_access_key_id: Some(env_var("AWS_ACCESS_KEY_ID").unwrap()),
            aws_secret_access_key: Some(env_var("AWS_SECRET_ACCESS_KEY").unwrap()),
            aws_region: Some(Region::EuCentral1),
        };
        let proc_env = cfg
            .into_vault()
            .unwrap()
            .download_json("kvenv-tests/prefixed-1")
            .unwrap();
        assert_eq!(
            vec![
                env!("INTEGRATION_TESTS_A", "work1"),
                env!("INTEGRATION_TESTS_B", "work2"),
            ],
            proc_env
        );
    }

    #[cfg(feature = "integration-tests")]
    #[test]
    fn integration_tests_prefixed() {
        use std::env::var as env_var;
        let cfg = AwsConfig {
            enabled: true,
            aws_access_key_id: Some(env_var("AWS_ACCESS_KEY_ID").unwrap()),
            aws_secret_access_key: Some(env_var("AWS_SECRET_ACCESS_KEY").unwrap()),
            aws_region: Some(Region::EuCentral1),
        };
        let proc_env = cfg
            .into_vault()
            .unwrap()
            .download_prefixed("kvenv-tests/prefixed-")
            .unwrap();
        assert_eq!(
            vec![
                env!("INTEGRATION_TESTS_A", "work1"),
                env!("INTEGRATION_TESTS_B", "work2"),
                env!("INTEGRATION_TESTS_C", "work3")
            ],
            proc_env
        );
    }
}
