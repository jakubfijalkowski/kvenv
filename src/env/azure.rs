use std::sync::Arc;

use azure_core::auth::TokenCredential;
use azure_identity::{
    ClientSecretCredential, DefaultAzureCredentialBuilder, TokenCredentialOptions,
};
use azure_security_keyvault::prelude::*;
use clap::{arg, command, ArgGroup, Args};
use futures::future::try_join_all;
use futures::stream::StreamExt;
use serde_json::Value;
use thiserror::Error;

use super::{
    convert::{convert_env_name, decode_env_from_json},
    Vault, VaultConfig,
};

#[derive(Args, Debug)]
#[command(group = ArgGroup::new("keyvault"))]
pub struct AzureConfig {
    /// Use Azure Key Vault.
    #[arg(
        name = "azure",
        long = "azure",
        group = "cloud",
        requires = "keyvault",
        display_order = 200
    )]
    enabled: bool,

    #[command(flatten)]
    credential: AzureCredential,

    /// [Azure] The name of Azure KeyVault (in the public cloud) where the secret lives. Cannot be
    /// used with `keyvault-url`.
    #[arg(
        long,
        env = "AZURE_KEYVAULT_NAME",
        group = "keyvault",
        display_order = 201
    )]
    azure_keyvault_name: Option<String>,

    /// [Azure] The URL to the Azure KeyVault where the secret lives. Cannot be used with
    /// `keyvault-name`.
    #[arg(
        long,
        env = "AZURE_KEYVAULT_URL",
        group = "keyvault",
        display_order = 202
    )]
    azure_keyvault_url: Option<String>,
}

#[derive(Args, Debug, Default)]
pub struct AzureCredential {
    /// [Azure] The tenant id of the service principal used for authorization.
    #[arg(long, env = "AZURE_TENANT_ID", display_order = 203)]
    azure_tenant_id: Option<String>,

    /// [Azure] The application id of the service principal used for authorization.
    #[arg(long, env = "AZURE_CLIENT_ID", display_order = 204)]
    azure_client_id: Option<String>,

    /// [Azure] The secret of the service principal used for authorization.
    #[arg(
        long,
        env = "AZURE_CLIENT_SECRET",
        hide_env_values = true,
        display_order = 205
    )]
    azure_client_secret: Option<String>,
}

#[derive(Error, Debug)]
pub enum AzureError {
    #[error("Azure configuration error")]
    ConfigurationError(#[source] anyhow::Error),
    #[error("Azure configuration error")]
    AzureError(#[source] azure_core::Error),
}

pub struct AzureVault {
    kv_address: String,
    credential: Arc<dyn TokenCredential>,
}

pub type Result<T, E = AzureError> = std::result::Result<T, E>;

impl AzureCredential {
    fn is_valid(&self) -> bool {
        self.azure_tenant_id.is_some()
            && self.azure_client_id.is_some()
            && self.azure_client_secret.is_some()
    }

    fn validate(&self) -> Result<()> {
        let has_some = self.azure_tenant_id.is_some()
            || self.azure_client_id.is_some()
            || self.azure_client_secret.is_some();
        if has_some && !self.is_valid() {
            Err(AzureError::ConfigurationError(anyhow::Error::msg(
                "if you want to use CLI-passed credentials, all need to be specified",
            )))
        } else {
            Ok(())
        }
    }

    fn to_credential(&self) -> Result<Arc<dyn TokenCredential>> {
        self.validate()?;
        if self.is_valid() {
            let creds = ClientSecretCredential::new(
                azure_core::new_http_client(),
                self.azure_tenant_id.clone().unwrap(),
                self.azure_client_id.clone().unwrap(),
                self.azure_client_secret.clone().unwrap(),
                TokenCredentialOptions::default(),
            );
            Ok(Arc::new(creds))
        } else {
            let creds = DefaultAzureCredentialBuilder::new()
                .exclude_environment_credential()
                .build();
            Ok(Arc::new(creds))
        }
    }
}

impl AzureConfig {
    fn get_kv_address(&self) -> Result<String> {
        if let Some(url) = &self.azure_keyvault_url {
            Ok(url.to_string())
        } else if let Some(name) = &self.azure_keyvault_name {
            Ok(format!("https://{}.vault.azure.net", name))
        } else {
            Err(AzureError::ConfigurationError(anyhow::Error::msg(
                "configuration is invalid (Clap should not validate that)",
            )))
        }
    }
}

impl VaultConfig for AzureConfig {
    type Vault = AzureVault;

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn into_vault(self) -> anyhow::Result<Self::Vault> {
        let kv_address = self.get_kv_address()?;
        let credential = self.credential.to_credential()?;
        Ok(AzureVault {
            kv_address,
            credential,
        })
    }
}

impl AzureVault {
    fn get_client(&self) -> Result<SecretClient> {
        SecretClient::new(&self.kv_address, self.credential.clone()).map_err(AzureError::AzureError)
    }

    fn strip_prefix(name: &str) -> &str {
        let idx = name.rfind('/').unwrap();
        &name[(idx + 1)..]
    }
}

impl Vault for AzureVault {
    #[tokio::main]
    async fn download_prefixed(&self, prefix: &str) -> anyhow::Result<Vec<(String, String)>> {
        let client = self.get_client()?;

        let secrets = client
            .list_secrets()
            .into_stream()
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .map_err(AzureError::AzureError)?;
        let secrets: Vec<_> = secrets
            .into_iter()
            .flat_map(|x| x.value.into_iter().map(|x| x.id))
            .map(|x| AzureVault::strip_prefix(&x).to_string())
            .filter(|x| x.starts_with(&prefix))
            .collect();
        let env_names = secrets
            .iter()
            .map(|x| convert_env_name(prefix, x))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let env_values = secrets.iter().map(|s| {
            let client = self.get_client();
            async move {
                client?
                    .get(s)
                    .into_future()
                    .await
                    .map_err(AzureError::AzureError)
            }
        });
        let env_values = try_join_all(env_values).await?.into_iter().map(|x| x.value);
        let from_kv = env_names.into_iter().zip(env_values.into_iter()).collect();
        Ok(from_kv)
    }

    #[tokio::main]
    async fn download_json(&self, secret_name: &str) -> anyhow::Result<Vec<(String, String)>> {
        let client = self.get_client()?;
        let secret = client
            .get(secret_name)
            .into_future()
            .await
            .map_err(AzureError::AzureError)?;
        let value: Value = serde_json::from_str(&secret.value)?;
        decode_env_from_json(secret_name, value)
    }
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn get_kv_address_raw_url() {
        let cfg = AzureConfig {
            enabled: true,
            credential: AzureCredential::default(),
            azure_keyvault_url: Some("url".to_string()),
            azure_keyvault_name: None,
        };

        assert_eq!("url", cfg.get_kv_address().unwrap());
    }

    #[test]
    fn get_kv_address_name() {
        let cfg = AzureConfig {
            enabled: true,
            credential: AzureCredential::default(),
            azure_keyvault_name: Some("name".to_string()),
            azure_keyvault_url: None,
        };

        assert_eq!(
            "https://name.vault.azure.net",
            cfg.get_kv_address().unwrap()
        );
    }

    #[cfg(feature = "integration-tests")]
    #[test]
    fn integration_tests_single_value() {
        use std::env::var as env_var;
        let cfg = AzureConfig {
            enabled: true,
            credential: AzureCredential {
                azure_tenant_id: Some(env_var("KVENV_TENANT_ID").unwrap()),
                azure_client_id: Some(env_var("KVENV_CLIENT_ID").unwrap()),
                azure_client_secret: Some(env_var("KVENV_CLIENT_SECRET").unwrap()),
            },
            azure_keyvault_name: Some(env_var("KVENV_KEYVAULT_NAME").unwrap()),
            azure_keyvault_url: None,
        };
        let proc_env = cfg
            .into_vault()
            .unwrap()
            .download_json("integ-tests")
            .unwrap();
        assert_eq!(vec![env!("INTEGRATION_TESTS", "work")], proc_env);
    }

    #[cfg(feature = "integration-tests")]
    #[test]
    fn integration_tests_prefixed() {
        use std::env::var as env_var;
        let cfg = AzureConfig {
            enabled: true,
            credential: AzureCredential {
                azure_tenant_id: Some(env_var("KVENV_TENANT_ID").unwrap()),
                azure_client_id: Some(env_var("KVENV_CLIENT_ID").unwrap()),
                azure_client_secret: Some(env_var("KVENV_CLIENT_SECRET").unwrap()),
            },
            azure_keyvault_name: Some(env_var("KVENV_KEYVAULT_NAME").unwrap()),
            azure_keyvault_url: None,
        };
        let proc_env = cfg
            .into_vault()
            .unwrap()
            .download_prefixed("prefixed-")
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
