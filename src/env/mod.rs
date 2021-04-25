use anyhow::Result;
use clap::{ArgGroup, Clap};

mod azure;
mod convert;
mod google;
mod process_env;

use azure::*;
use google::GoogleConfig;
pub use process_env::ProcessEnv;

pub trait Vault {
    fn download_prefixed(&self, prefix: &str) -> Result<Vec<(String, String)>>;
    fn download_json(&self, secret_name: &str) -> Result<Vec<(String, String)>>;
}

pub trait VaultConfig {
    type Vault: Vault;
    fn into_vault(self) -> Result<(Self::Vault, DataConfig)>;
}

#[derive(Clap, Debug)]
#[clap(group = ArgGroup::new("secret").required(true))]
pub struct DataConfig {
    /// The name of the secret with the environment defined. Cannot be used along `secret-prefix`.
    #[clap(short = 'n', long, env = "KVENV_SECRET_NAME", group = "secret")]
    secret_name: Option<String>,

    /// The prefix of secrets with the environment variables. Cannot be used along `secret-name`.
    #[clap(short = 's', long, env = "KVENV_SECRET_PREFIX", group = "secret")]
    secret_prefix: Option<String>,

    /// If set, `kvenv` will use OS's environment at the point in time when the environment is
    /// downloaded.
    #[clap(short = 'e', long)]
    snapshot_env: bool,

    /// Environment variables that should be masked by the subsequent calls to `with`.
    #[clap(short, long)]
    mask: Vec<String>,
}

impl Default for DataConfig {
    fn default() -> Self {
        Self {
            secret_name: None,
            secret_prefix: None,
            snapshot_env: false,
            mask: vec![],
        }
    }
}

#[derive(Clap, Debug)]
enum CloudConfig {
    Azure(AzureConfig),
    Google(GoogleConfig),
}

#[derive(Clap, Debug)]
pub struct EnvConfig {
    #[clap(subcommand)]
    cloud: CloudConfig,
}

impl EnvConfig {
    fn into_run_config(self) -> Result<(Box<dyn Vault>, DataConfig)> {
        match self.cloud {
            CloudConfig::Azure(a) => Self::box_vault(a.into_vault()?),
            CloudConfig::Google(g) => Self::box_vault(g.into_vault()?),
        }
    }

    fn box_vault<T: Vault + 'static>(v: (T, DataConfig)) -> Result<(Box<dyn Vault>, DataConfig)> {
        let (v, c) = v;
        Ok((Box::new(v), c))
    }
}

pub fn download_env(cfg: EnvConfig) -> Result<ProcessEnv> {
    let (vault, cfg) = cfg.into_run_config()?;
    let from_kv = if cfg.secret_name.is_some() {
        vault.download_json(&cfg.secret_name.unwrap())?
    } else if cfg.secret_prefix.is_some() {
        vault.download_prefixed(&cfg.secret_prefix.unwrap())?
    } else {
        unreachable!()
    };
    Ok(ProcessEnv::new(from_kv, cfg.mask, cfg.snapshot_env))
}
