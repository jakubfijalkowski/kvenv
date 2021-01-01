use azure_identity::token_credentials::{ClientSecretCredential, TokenCredentialOptions};
use azure_key_vault::{KeyVaultClient, KeyVaultError};
use clap::{ArgSettings, Clap};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EnvLoadError {
    #[error("cannot load secret from keyvault")]
    CannotLoadSecret(#[from] KeyVaultError),
    #[error("invalid format - the object is not a map or not all keys are strings")]
    InvalidSecretFormat,
    #[error("cannot deserialize the env file")]
    CannotDeserialize(#[from] serde_json::error::Error),
}

type Result<T> = std::result::Result<T, EnvLoadError>;

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

    /// If set, `kvenv` will use OS's environment at this point in time.
    #[clap(short = 'e', long)]
    snapshot_env: bool,

    /// Environment variables that should be masked by the subsequent calls to `with`.
    #[clap(short, long)]
    mask: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
enum OsEnv {
    Persisted(Vec<(String, String)>),
    Fresh(Vec<(String, String)>),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProcessEnv {
    #[serde(
        skip_serializing_if = "OsEnv::should_not_persist",
        default = "OsEnv::empty"
    )]
    from_env: OsEnv,
    from_kv: Vec<(String, String)>,
    masked: Vec<String>,
}

impl OsEnv {
    fn new(persisted: bool) -> Self {
        let env = std::env::vars().collect();
        if persisted {
            Self::Persisted(env)
        } else {
            Self::Fresh(env)
        }
    }

    fn empty() -> Self {
        Self::Fresh(std::env::vars().collect())
    }

    fn should_not_persist(&self) -> bool {
        match self {
            OsEnv::Persisted(_) => false,
            _ => true,
        }
    }
}

impl IntoIterator for OsEnv {
    type Item = (String, String);
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            Self::Persisted(p) => p.into_iter(),
            Self::Fresh(p) => p.into_iter(),
        }
    }
}

impl ProcessEnv {
    fn new(from_kv: Vec<(String, String)>, masked: Vec<String>, snapshot_env: bool) -> Self {
        Self {
            from_env: OsEnv::new(snapshot_env),
            from_kv,
            masked,
        }
    }

    pub fn from_str(s: &str) -> Result<Self> {
        let result = serde_json::from_str(s).map_err(EnvLoadError::CannotDeserialize)?;
        Ok(result)
    }

    pub fn to_string(&self) -> String {
        serde_json::to_string(self).unwrap()
    }

    pub fn to_writer<W: std::io::Write>(&self, w: W) -> serde_json::Result<()> {
        serde_json::to_writer(w, self)
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

async fn download_env(cfg: EnvConfig) -> Result<ProcessEnv> {
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
            Ok(ProcessEnv::new(from_kv, cfg.mask, cfg.snapshot_env))
        }
        _ => Err(EnvLoadError::InvalidSecretFormat),
    }
}

#[tokio::main]
pub async fn prepare_env(cfg: EnvConfig) -> Result<ProcessEnv> {
    download_env(cfg).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_env() {
        let env = ProcessEnv {
            from_env: OsEnv::Persisted(vec![
                ("A".to_string(), "ENV".to_string()),
                ("B".to_string(), "ENV".to_string()),
                ("C".to_string(), "ENV".to_string()),
            ]),
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

    #[test]
    fn serialization_persisted() {
        let persisted = |env, kv, masked| ProcessEnv {
            from_env: OsEnv::Persisted(env),
            from_kv: kv,
            masked,
        };

        let test = |env: &ProcessEnv| {
            let serialized = env.to_string();
            ProcessEnv::from_str(&serialized).unwrap()
        };

        let env = vec![("A".to_string(), "B".to_string())];
        let kv = vec![("C".to_string(), "D".to_string())];
        let masked = vec!["E".to_string()];
        let proc_env = persisted(env.clone(), kv.clone(), masked.clone());

        let serialized = test(&proc_env);

        assert_eq!(masked, serialized.masked);
        assert_eq!(kv, serialized.from_kv);
        assert!(matches!(serialized.from_env, OsEnv::Persisted(_)));
        assert_eq!(env, serialized.from_env.into_iter().collect::<Vec<_>>());
    }

    #[test]
    fn serialization_fresh() {
        let fresh = |kv, masked| ProcessEnv {
            from_env: OsEnv::Fresh(vec![("Ignore".to_string(), "me".to_string())]),
            from_kv: kv,
            masked,
        };

        let test = |env: &ProcessEnv| {
            let serialized = env.to_string();
            ProcessEnv::from_str(&serialized).unwrap()
        };

        let kv = vec![("C".to_string(), "D".to_string())];
        let masked = vec!["E".to_string()];
        let proc_env = fresh(kv.clone(), masked.clone());

        let serialized = test(&proc_env);

        assert_eq!(masked, serialized.masked);
        assert_eq!(kv, serialized.from_kv);
        assert!(matches!(serialized.from_env, OsEnv::Fresh(_)));
        assert_eq!(
            std::env::vars().collect::<Vec<_>>(),
            serialized.from_env.into_iter().collect::<Vec<_>>()
        );
    }
}
