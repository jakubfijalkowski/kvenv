use azure_core::TokenCredential;
use azure_identity::token_credentials::{
    AzureCliCredential, ClientSecretCredential, DefaultCredential, ManagedIdentityCredential,
    TokenCredentialOptions,
};
use azure_key_vault::{KeyClient, KeyVaultError};
use clap::{ArgGroup, ArgSettings, Clap};
use futures::future::try_join_all;
use serde_json::Value;
use thiserror::Error;

use super::{
    convert::{convert_env_name, decode_env_from_json},
    VaultConfig,
};

#[derive(Clap, Debug)]
#[clap()]
pub struct AzureCredential {
    /// The tenant id of the service principal used for authorization.
    #[clap(short, long, env = "AZURE_TENANT_ID")]
    tenant_id: Option<String>,

    /// The application id of the service principal used for authorization.
    #[clap(short = 'c', long, env = "AZURE_CLIENT_ID")]
    client_id: Option<String>,

    /// The secret of the service principal used for authorization.
    #[clap(short = 's', long, env = "AZURE_CLIENT_SECRET", setting = ArgSettings::HideEnvValues)]
    client_secret: Option<String>,
}

#[derive(Clap, Debug)]
#[clap(group = ArgGroup::new("keyvault").required(true))]
pub struct AzureConfig {
    #[clap(flatten)]
    credential: AzureCredential,

    /// The name of Azure KeyVault (in the public cloud) where the secret lives. Cannot be used
    /// with `keyvault-url`.
    #[clap(short = 'k', long, env = "AZURE_KEYVAULT_NAME", group = "keyvault")]
    keyvault_name: Option<String>,

    /// The URL to the Azure KeyVault where the secret lives. Cannot be used with `keyvault-name`.
    #[clap(short = 'u', long, env = "AZURE_KEYVAULT_URL", group = "keyvault")]
    keyvault_url: Option<String>,
}

#[derive(Error, Debug)]
pub enum AzureError {
    #[error("Azure configuration error")]
    ConfigurationError(#[source] anyhow::Error),
    #[error("cannot load secret from keyvault")]
    KeyVaultError(#[source] KeyVaultError),
}

pub type Result<T, E = AzureError> = std::result::Result<T, E>;

impl AzureCredential {
    fn is_valid(&self) -> bool {
        self.tenant_id.is_some() && self.client_id.is_some() && self.client_secret.is_some()
    }

    fn validate(&self) -> Result<()> {
        let has_some =
            self.tenant_id.is_some() || self.client_id.is_some() || self.client_secret.is_some();
        if has_some && !self.is_valid() {
            Err(AzureError::ConfigurationError(anyhow::Error::msg(
                "if you want to use CLI-passed credentials, all need to be specified",
            )))
        } else {
            Ok(())
        }
    }

    fn to_credential(&self) -> Result<DefaultCredential> {
        self.validate()?;
        if self.is_valid() {
            let creds = ClientSecretCredential::new(
                self.tenant_id.clone().unwrap(),
                self.client_id.clone().unwrap(),
                self.client_secret.clone().unwrap(),
                TokenCredentialOptions::default(),
            );
            Ok(DefaultCredential::with_sources(vec![Box::new(creds)]))
        } else {
            Ok(DefaultCredential::with_sources(vec![
                Box::new(ManagedIdentityCredential {}),
                Box::new(AzureCliCredential {}),
            ]))
        }
    }
}

impl Default for AzureCredential {
    fn default() -> Self {
        Self {
            tenant_id: None,
            client_id: None,
            client_secret: None,
        }
    }
}

struct KeyClientData {
    kv_address: String,
    credential: DefaultCredential,
}

impl AzureConfig {
    fn get_kv_address(&self) -> Result<String> {
        if let Some(url) = &self.keyvault_url {
            Ok(url.to_string())
        } else if let Some(name) = &self.keyvault_name {
            Ok(format!("https://{}.vault.azure.net", name))
        } else {
            Err(AzureError::ConfigurationError(anyhow::Error::msg(
                "configuration is invalid (Clap should not validate that)",
            )))
        }
    }

    fn to_client(&self) -> Result<KeyClientData> {
        let kv_address = self.get_kv_address()?;
        let credential = self.credential.to_credential()?;
        Ok(KeyClientData {
            kv_address,
            credential,
        })
    }
}

impl KeyClientData {
    fn get_client<'a>(&'a self) -> Result<KeyClient<'a, DefaultCredential>> {
        KeyClient::new(&self.kv_address, &self.credential).map_err(AzureError::ConfigurationError)
    }
}

impl VaultConfig for AzureConfig {
    #[tokio::main]
    async fn download_prefixed(&self, prefix: &str) -> anyhow::Result<Vec<(String, String)>> {
        let client_data = self.to_client()?;
        let mut client = client_data.get_client()?;

        let secrets = client
            .list_secrets()
            .await
            .map_err(AzureError::KeyVaultError)?;
        let secrets: Vec<_> = secrets
            .iter()
            .filter(|x| x.name().starts_with(&prefix))
            .collect();
        let env_names = secrets
            .iter()
            .map(|x| convert_env_name(&prefix, x.name()))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let env_values = secrets.iter().map(|s| {
            let client = client_data.get_client();
            async move {
                client?
                    .get_secret(s.name())
                    .await
                    .map_err(AzureError::KeyVaultError)
            }
        });
        let env_values = try_join_all(env_values)
            .await?
            .into_iter()
            .map(|x| x.value().to_owned());
        let from_kv = env_names.into_iter().zip(env_values.into_iter()).collect();
        Ok(from_kv)
    }

    #[tokio::main]
    async fn download_json(&self, secret_name: &str) -> anyhow::Result<Vec<(String, String)>> {
        let client_data = self.to_client()?;
        let mut client = client_data.get_client()?;
        let secret = client
            .get_secret(secret_name)
            .await
            .map_err(AzureError::KeyVaultError)?;
        let value: Value = serde_json::from_str(secret.value())?;
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
            credential: AzureCredential::default(),
            keyvault_url: Some("url".to_string()),
            keyvault_name: None,
        };

        assert_eq!("url", cfg.get_kv_address().unwrap());
    }

    #[test]
    fn get_kv_address_name() {
        let cfg = AzureConfig {
            credential: AzureCredential::default(),
            keyvault_name: Some("name".to_string()),
            keyvault_url: None,
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
            credential: AzureCredential {
                tenant_id: Some(env_var("KVENV_TENANT_ID").unwrap()),
                client_id: Some(env_var("KVENV_CLIENT_ID").unwrap()),
                client_secret: Some(env_var("KVENV_CLIENT_SECRET").unwrap()),
            },
            keyvault_name: Some(env_var("KVENV_KEYVAULT_NAME").unwrap()),
            keyvault_url: None,
        };
        dbg!(&cfg);
        let proc_env = cfg
            .download_json(&env_var("KVENV_SECRET_NAME").unwrap())
            .unwrap();
        assert_eq!(vec![env!("INTEGRATION_TESTS", "work")], proc_env);
    }

    #[cfg(feature = "integration-tests")]
    #[test]
    fn integration_tests_prefixed() {
        use std::env::var as env_var;
        let cfg = AzureConfig {
            credential: AzureCredential {
                tenant_id: Some(env_var("KVENV_TENANT_ID").unwrap()),
                client_id: Some(env_var("KVENV_CLIENT_ID").unwrap()),
                client_secret: Some(env_var("KVENV_CLIENT_SECRET").unwrap()),
            },
            keyvault_name: Some(env_var("KVENV_KEYVAULT_NAME").unwrap()),
            keyvault_url: None,
        };
        let proc_env = cfg
            .download_prefixed(&env_var("KVENV_SECRET_PREFIX").unwrap())
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
