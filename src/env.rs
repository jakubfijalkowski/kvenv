use azure_identity::token_credentials::{ClientSecretCredential, TokenCredentialOptions};
use azure_key_vault::{KeyClient, KeyVaultError};
use clap::{ArgGroup, ArgSettings, Clap};
use futures::future::try_join_all;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EnvLoadError {
    #[error("configuration error")]
    ConfigurationError(anyhow::Error),
    #[error("cannot load secret from keyvault")]
    CannotLoadSecret(#[from] KeyVaultError),
    #[error("invalid format - the object is not a map or not all keys are strings")]
    InvalidSecretFormat,
    #[error("cannot deserialize the env file")]
    CannotDeserialize(#[from] serde_json::error::Error),
}

type Result<T> = std::result::Result<T, EnvLoadError>;

#[derive(Clap, Debug)]
#[clap(group = ArgGroup::new("secret").required(true))]
pub struct EnvConfig {
    /// The tenant id of the service principal used for authorization.
    #[clap(short, long, env = "KVENV_TENANT_ID")]
    tenant_id: String,

    /// The application id of the service principal used for authorization.
    #[clap(short = 'c', long, env = "KVENV_CLIENT_ID")]
    client_id: String,

    /// The secret of the service principal used for authorization.
    #[clap(short = 's', long, env = "KVENV_CLIENT_SECRET", setting = ArgSettings::HideEnvValues)]
    client_secret: String,

    /// The name of Azure KeyVault where the secret lives.
    #[clap(short, long, env = "KVENV_KEYVAULT_NAME")]
    keyvault_name: String,

    /// The name of the secret with the environment defined. Cannot be used along `secret-prefix`.
    #[clap(short = 'n', long, env = "KVENV_SECRET_NAME", group = "secret")]
    secret_name: Option<String>,

    /// The prefix of secrets with the environment variables. Cannot be used along `secret-name`.
    #[clap(short = 'p', long, env = "KVENV_SECRET_PREFIX", group = "secret")]
    secret_prefix: Option<String>,

    /// If set, `kvenv` will use OS's environment at the point in time when the environment is
    /// downloaded.
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
        matches!(self, OsEnv::Fresh(_))
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

    pub fn from_reader<R: std::io::Read>(rdr: R) -> serde_json::Result<Self> {
        serde_json::from_reader(rdr)
    }

    pub fn to_writer<W: std::io::Write>(&self, w: W) -> serde_json::Result<()> {
        serde_json::to_writer(w, self)
    }

    pub fn into_env(self) -> HashMap<String, String> {
        let mut map: HashMap<_, _> = self.from_env.into_iter().collect();
        map.extend(self.from_kv);
        for m in self.masked {
            map.remove(&m);
        }
        map
    }
}

#[cfg(test)]
impl ProcessEnv {
    pub fn fresh(
        from_env: Vec<(String, String)>,
        from_kv: Vec<(String, String)>,
        masked: Vec<String>,
    ) -> Self {
        Self {
            from_env: OsEnv::Fresh(from_env),
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
}

fn as_valid_env_name(name: String) -> Result<String> {
    let is_valid = |c: char| c.is_ascii_alphanumeric() || c == '_';
    if !name.is_empty()
        && name.chars().all(is_valid)
        && name
            .chars()
            .next()
            .map_or(false, |c| c.is_ascii_alphabetic())
    {
        Ok(name)
    } else {
        Err(EnvLoadError::InvalidSecretFormat)
    }
}

fn value_as_string(v: Value) -> Result<String> {
    match v {
        Value::String(s) => Ok(s),
        Value::Bool(b) => Ok(format!("{}", b)),
        Value::Number(n) => Ok(format!("{}", n)),
        Value::Null => Ok("null".to_string()),
        _ => Err(EnvLoadError::InvalidSecretFormat),
    }
}

fn convert_env_name(prefix: &str, name: &str) -> Result<String> {
    let name = name[prefix.len()..].replace("-", "_");
    as_valid_env_name(name)
}

fn decode_env_from_json(value: Value) -> Result<Vec<(String, String)>> {
    match value {
        Value::Object(m) => m
            .into_iter()
            .map(|(k, v)| Ok((as_valid_env_name(k)?, value_as_string(v)?)))
            .collect(),
        _ => Err(EnvLoadError::InvalidSecretFormat),
    }
}

fn get_kv_address(name: &str) -> String {
    format!("https://{}.vault.azure.net", name)
}

async fn download_env_single(cfg: EnvConfig) -> Result<ProcessEnv> {
    let creds = ClientSecretCredential::new(
        cfg.tenant_id,
        cfg.client_id,
        cfg.client_secret,
        TokenCredentialOptions::default(),
    );
    let mut client = KeyClient::new(&get_kv_address(&cfg.keyvault_name), &creds)
        .map_err(EnvLoadError::ConfigurationError)?;
    let secret = client
        .get_secret(&cfg.secret_name.unwrap())
        .await
        .map_err(EnvLoadError::CannotLoadSecret)?;
    let value: Value = serde_json::from_str(secret.value())?;
    let from_kv = decode_env_from_json(value)?;
    Ok(ProcessEnv::new(from_kv, cfg.mask, cfg.snapshot_env))
}

async fn download_env_prefixed(cfg: EnvConfig) -> Result<ProcessEnv> {
    let creds = ClientSecretCredential::new(
        cfg.tenant_id,
        cfg.client_id,
        cfg.client_secret,
        TokenCredentialOptions::default(),
    );
    let kv_name = cfg.keyvault_name;
    let prefix = cfg.secret_prefix.unwrap();
    let mut client = KeyClient::new(&get_kv_address(&kv_name), &creds)
        .map_err(EnvLoadError::ConfigurationError)?;
    let secrets = client
        .list_secrets()
        .await
        .map_err(EnvLoadError::CannotLoadSecret)?;
    let secrets: Vec<_> = secrets
        .iter()
        .filter(|x| x.name().starts_with(&prefix))
        .collect();
    let env_names = secrets
        .iter()
        .map(|x| convert_env_name(&prefix, x.name()))
        .collect::<Result<Vec<_>>>()?;
    let env_values = secrets.iter().map(|s| {
        let client = KeyClient::new(&get_kv_address(&kv_name), &creds);
        async move {
            client
                .map_err(EnvLoadError::ConfigurationError)?
                .get_secret(s.name())
                .await
                .map_err(EnvLoadError::CannotLoadSecret)
        }
    });
    let env_values = try_join_all(env_values)
        .await?
        .into_iter()
        .map(|x| x.value().to_owned());
    let from_kv = env_names.into_iter().zip(env_values.into_iter()).collect();
    Ok(ProcessEnv::new(from_kv, cfg.mask, cfg.snapshot_env))
}

pub async fn download_env(cfg: EnvConfig) -> Result<ProcessEnv> {
    if cfg.secret_name.is_some() {
        download_env_single(cfg).await
    } else if cfg.secret_prefix.is_some() {
        download_env_prefixed(cfg).await
    } else {
        unreachable!()
    }
}

#[tokio::main]
pub async fn download_env_sync(cfg: EnvConfig) -> Result<ProcessEnv> {
    download_env(cfg).await
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    macro_rules! env {
        ($a:expr) => {
            $a.to_string()
        };
        ($a:expr, $b:expr) => {
            ($a.to_string(), $b.to_string())
        };
    }

    macro_rules! assert_invalid_secret {
        ($a:expr) => {
            assert!(matches!($a, Err(EnvLoadError::InvalidSecretFormat)));
        };
    }

    #[test]
    fn as_valid_env_name_correct() {
        macro_rules! assert_convert {
            ($a:expr) => {
                assert_eq!($a, as_valid_env_name($a.to_string()).unwrap());
            };
        }

        assert_convert!("abc");
        assert_convert!("abc123");
        assert_convert!("ab_12_ab");
    }

    #[test]
    fn as_valid_env_name_invalid() {
        macro_rules! assert_fail {
            ($a:expr) => {
                assert_invalid_secret!(as_valid_env_name($a.to_string()));
            };
        }

        assert_fail!("");
        assert_fail!("123abc");
        assert_fail!("ab!");
        assert_fail!("ab-ab");
    }

    #[test]
    fn value_as_string_for_normal_values() {
        macro_rules! assert_convert {
            ($a:expr, $b:expr) => {
                assert_eq!($a, value_as_string($b).unwrap());
            };
        }

        assert_convert!("abcd", json!("abcd"));
        assert_convert!("12", json!(12));
        assert_convert!("12.123", json!(12.123));
        assert_convert!("null", json!(null));
        assert_convert!("false", json!(false));
        assert_convert!("true", json!(true));
    }

    #[test]
    fn value_as_string_for_arrays_and_objects() {
        macro_rules! assert_fail {
            ($a:expr) => {
                assert_invalid_secret!(value_as_string($a));
            };
        }

        assert_fail!(json!({ "a": 123 }));
        assert_fail!(json!([1, 2]));
    }

    #[test]
    fn decode_env_from_json_correct_values() {
        // Overkill, but looks quite awesome :)
        macro_rules! assert_decode {
            ($a:expr, $($name:ident = $value:expr),*) => {
                #[allow(unused_variables, unused_mut)]
                {
                    let decoded = decode_env_from_json($a).unwrap();
                    let len = decoded.len();
                    let mapped = decoded.into_iter().collect::<HashMap<_, _>>();
                    let mut total = 0;
                    $(
                        total += 1;
                        let name = stringify!($name);
                        let value = $value.to_string();
                        assert_eq!(Some(&value), mapped.get(&name[..]));
                    )*
                    assert_eq!(total, len);
                }
            };
        }

        assert_decode!(json!({}),);
        assert_decode!(json!({"a": 1}), a = "1");
        assert_decode!(json!({"a": 1, "b": true}), a = "1", b = "true");
        assert_decode!(
            json!({"a": 1, "b": true, "c": "test"}),
            a = "1",
            b = "true",
            c = "test"
        );
    }

    #[test]
    fn decode_env_from_json_invalid() {
        macro_rules! assert_fail {
            ($a:expr) => {
                assert_invalid_secret!(decode_env_from_json($a));
            };
        }

        assert_fail!(json!([1, 2]));
        assert_fail!(json!("test"));
        assert_fail!(json!(false));
        assert_fail!(json!(true));
        assert_fail!(json!({"a!": 1}));
        assert_fail!(json!({"1a": 1}));
        assert_fail!(json!({"a": {"b": 1}}));
    }

    #[test]
    fn convert_env_name_converts_names() {
        macro_rules! assert_convert {
            ($a:expr, $b:expr) => {
                assert_convert!("", $a, $b);
            };
            ($prefix:expr, $a:expr, $b:expr) => {
                assert_eq!($a, convert_env_name($prefix, $b).unwrap());
            };
        }

        assert_convert!("abc", "abc");
        assert_convert!("abc123", "abc123");
        assert_convert!("abc_123", "abc-123");
        assert_convert!("abc__123", "abc--123");
        assert_convert!("abc_123", "abc_123");

        assert_convert!("zxc", "abc", "zxcabc");
        assert_convert!("zxc", "abc", "abcabc");
    }

    #[test]
    fn convert_env_name_invalid() {
        macro_rules! assert_fail {
            ($a:expr) => {
                assert_fail!("", $a);
            };
            ($prefix:expr, $a:expr) => {
                assert_invalid_secret!(convert_env_name($prefix, $a));
            };
        }

        assert_fail!("");
        assert_fail!("!");
        assert_fail!("abc!");
        assert_fail!("123abc");
        assert_fail!("abc", "abc");
    }

    #[test]
    fn into_env() {
        let env = ProcessEnv {
            from_env: OsEnv::Persisted(vec![env!("A", "ENV"), env!("B", "ENV"), env!("C", "ENV")]),
            from_kv: vec![
                env!("A", "KV"),
                env!("B", "KV"),
                env!("D", "KV"),
                env!("E", "KV"),
            ],
            masked: vec![env!("B"), env!("E")],
        };

        let env = env.into_env();

        assert_eq!(Some(&env!("KV")), env.get("A"));
        assert_eq!(None, env.get("B"));
        assert_eq!(Some(&env!("ENV")), env.get("C"));
        assert_eq!(Some(&env!("KV")), env.get("D"));
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

        let env = vec![env!("A", "B")];
        let kv = vec![env!("C", "D")];
        let masked = vec![env!("E")];
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
            from_env: OsEnv::Fresh(vec![env!("Ignore", "me")]),
            from_kv: kv,
            masked,
        };

        let test = |env: &ProcessEnv| {
            let serialized = env.to_string();
            ProcessEnv::from_str(&serialized).unwrap()
        };

        let kv = vec![env!("C", "D")];
        let masked = vec![env!("E")];
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

    #[test]
    fn kv_address() {
        assert_eq!("https://test.vault.azure.net", get_kv_address("test"));
    }

    #[cfg(feature = "integration-tests")]
    #[test]
    fn integration_tests_single_value() {
        use std::env::var as env_var;
        let cfg = EnvConfig {
            tenant_id: env_var("KVENV_TENANT_ID").unwrap(),
            client_id: env_var("KVENV_CLIENT_ID").unwrap(),
            client_secret: env_var("KVENV_CLIENT_SECRET").unwrap(),
            keyvault_name: env_var("KVENV_KEYVAULT_NAME").unwrap(),
            secret_name: Some(env_var("KVENV_SECRET_NAME").unwrap()),
            secret_prefix: None,
            snapshot_env: false,
            mask: vec![env!("A")],
        };
        let proc_env = download_env_sync(cfg).unwrap();
        assert_eq!(vec![env!("A")], proc_env.masked);
        assert_eq!(vec![env!("INTEGRATION_TESTS", "work")], proc_env.from_kv);
    }

    #[cfg(feature = "integration-tests")]
    #[test]
    fn integration_tests_prefixed() {
        use std::env::var as env_var;
        let cfg = EnvConfig {
            tenant_id: env_var("KVENV_TENANT_ID").unwrap(),
            client_id: env_var("KVENV_CLIENT_ID").unwrap(),
            client_secret: env_var("KVENV_CLIENT_SECRET").unwrap(),
            keyvault_name: env_var("KVENV_KEYVAULT_NAME").unwrap(),
            secret_name: None,
            secret_prefix: Some(env_var("KVENV_SECRET_PREFIX").unwrap()),
            snapshot_env: false,
            mask: vec![env!("A")],
        };
        let proc_env = download_env_sync(cfg).unwrap();
        assert_eq!(vec![env!("A")], proc_env.masked);
        assert_eq!(
            vec![
                env!("INTEGRATION_TESTS_A", "work1"),
                env!("INTEGRATION_TESTS_B", "work2"),
                env!("INTEGRATION_TESTS_C", "work3")
            ],
            proc_env.from_kv
        );
    }
}
