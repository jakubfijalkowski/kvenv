use anyhow::Result;
use clap::{ArgGroup, Clap};

mod azure;
mod convert;
mod process_env;

use azure::*;
pub use process_env::ProcessEnv;

pub trait VaultConfig {
    fn download_prefixed(&self, prefix: &str) -> Result<Vec<(String, String)>>;
    fn download_json(&self, secret_name: &str) -> Result<Vec<(String, String)>>;
}

#[derive(Clap, Debug)]
#[clap(group = ArgGroup::new("secret").required(true))]
pub struct DataConfig {
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

#[derive(Clap, Debug)]
#[clap()]
pub struct EnvConfig {
    #[clap(flatten)]
    azure: AzureConfig,

    #[clap(flatten)]
    data: DataConfig,
}

impl EnvConfig {
    fn to_run_config(self) -> (Box<dyn VaultConfig>, DataConfig) {
        (Box::new(self.azure), self.data)
    }
}

pub fn download_env(cfg: EnvConfig) -> Result<ProcessEnv> {
    let (vault, cfg) = cfg.to_run_config();
    let from_kv = if cfg.secret_name.is_some() {
        vault.download_json(&cfg.secret_name.unwrap())?
    } else if cfg.secret_prefix.is_some() {
        vault.download_prefixed(&cfg.secret_prefix.unwrap())?
    } else {
        unreachable!()
    };
    Ok(ProcessEnv::new(from_kv, cfg.mask, cfg.snapshot_env))
}
