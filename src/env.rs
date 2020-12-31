use anyhow::{Error, Result};
use azure_identity::token_credentials::{ClientSecretCredential, TokenCredentialOptions};
use azure_key_vault::{KeyVaultClient, KeyVaultError};
use clap::{ArgSettings, Clap};
use serde_json::Value;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EnvLoadError {
    #[error("cannot load secret from keyvault")]
    CannotLoadSecret(#[from] KeyVaultError),
    #[error("invalid format - the object is not a map or not all keys are strings")]
    InvalidSecretFormat,
}

#[derive(Clap, Debug)]
pub struct EnvConfig {
    /// The tenant id of the service principal used for authorization.
    #[clap(short, long, env = "ARM_TENANT_ID")]
    tenant_id: String,

    /// The application id of the service principal used for authorization.
    #[clap(short = 'c', long, env = "ARM_CLIENT_ID")]
    client_id: String,

    /// The secret of the service principal used for authorization.
    #[clap(short = 's', long, env = "ARM_CLIENT_SECRET", setting = ArgSettings::HideEnvValues)]
    client_secret: String,

    /// The name of Azure KeyVault where the secret lives.
    #[clap(short, long, env = "KEYVAULT_NAME")]
    keyvault_name: String,

    /// The name of secret with environment defined.
    #[clap(short = 'n', long, env = "SECRET_NAME")]
    secret_name: String,

    /// Environment variables that should be masked by the subsequent calls to `with`.
    #[clap(short, long)]
    mask: Vec<String>,
}

#[derive(Debug)]
pub struct ProcessEnv {
    from_env: Vec<(String, String)>,
    from_kv: Vec<(String, String)>,
    masked: Vec<String>,
}

impl ProcessEnv {
    fn new(from_kv: Vec<(String, String)>, masked: Vec<String>) -> Self {
        Self {
            from_env: std::env::vars().collect(),
            from_kv,
            masked,
        }
    }

    pub fn to_env(self) -> HashMap<String, String> {
        let mut map: HashMap<_, _> = self.from_env.into_iter().collect();
        map.extend(self.from_kv);
        for m in self.masked {
            map.remove(&m);
        }
        map
    }
}

fn can_put_to_env(v: &Value) -> bool {
    v.is_string() || v.is_boolean() || v.is_number() || v.is_null()
}

pub async fn prepare_env(cfg: EnvConfig) -> Result<ProcessEnv> {
    let creds = ClientSecretCredential::new(
        cfg.tenant_id,
        cfg.client_id,
        cfg.client_secret,
        TokenCredentialOptions::default(),
    );
    let mut client = KeyVaultClient::new(&creds, &cfg.keyvault_name);
    let secret = client
        .get_secret(&cfg.secret_name)
        .await
        .map_err(EnvLoadError::CannotLoadSecret)?;
    let secret = secret.value();
    let value: Value = serde_json::from_str(secret)?;
    match value {
        Value::Object(m) if m.iter().all(|(_, v)| can_put_to_env(v)) => {
            let from_kv: Vec<_> = m.into_iter().map(|(k, v)| (k, v.to_string())).collect();
            Ok(ProcessEnv::new(from_kv, cfg.mask))
        }
        _ => Err(Error::new(EnvLoadError::InvalidSecretFormat)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_env() {
        let env = ProcessEnv {
            from_env: vec![
                ("A".to_string(), "ENV".to_string()),
                ("B".to_string(), "ENV".to_string()),
                ("C".to_string(), "ENV".to_string()),
            ],
            from_kv: vec![
                ("A".to_string(), "KV".to_string()),
                ("B".to_string(), "KV".to_string()),
                ("D".to_string(), "KV".to_string()),
                ("E".to_string(), "KV".to_string()),
            ],
            masked: vec!["B".to_string(), "E".to_string()],
        };

        let env = env.to_env();

        assert_eq!(Some(&"KV".to_string()), env.get("A"));
        assert_eq!(None, env.get("B"));
        assert_eq!(Some(&"ENV".to_string()), env.get("C"));
        assert_eq!(Some(&"KV".to_string()), env.get("D"));
        assert_eq!(None, env.get("E"));
    }
}
