use anyhow::Result;
use clap::{arg, ArgGroup, Args};

#[cfg(feature = "aws")]
mod aws;
#[cfg(feature = "azure")]
mod azure;
#[cfg(feature = "google")]
mod google;
#[cfg(feature = "vault")]
mod vault;

mod convert;
mod process_env;

#[cfg(feature = "aws")]
use aws::AwsConfig;
#[cfg(feature = "azure")]
use azure::AzureConfig;
#[cfg(feature = "google")]
use google::GoogleConfig;
#[cfg(feature = "vault")]
use vault::HashicorpVaultConfig;

pub use process_env::ProcessEnv;

pub trait Vault {
    fn download_prefixed(&self, prefix: &str) -> Result<Vec<(String, String)>>;
    fn download_json(&self, secret_name: &str) -> Result<Vec<(String, String)>>;
}

pub trait VaultConfig {
    type Vault: Vault;
    fn is_enabled(&self) -> bool;
    fn into_vault(self) -> Result<Self::Vault>;
}

#[derive(Args, Debug, Default)]
#[command(group = ArgGroup::new("secret").required(true))]
pub struct DataConfig {
    /// The name of the secret with the environment defined. Cannot be used along `secret-prefix`.
    #[arg(
        short = 'n',
        long,
        env_os = "KVENV_SECRET_NAME",
        group = "secret",
        display_order = 1
    )]
    secret_name: Option<String>,

    /// The prefix of the secrets with the environment variables. Cannot be used along `secret-name`.
    #[arg(
        short = 's',
        long,
        env = "KVENV_SECRET_PREFIX",
        group = "secret",
        display_order = 2
    )]
    secret_prefix: Option<String>,

    /// If set, `kvenv` will use OS's environment at the point in time when the environment is
    /// downloaded.
    #[arg(short = 'e', long)]
    snapshot_env: bool,

    /// Environment variables that should be masked by the subsequent calls to `with`.
    #[arg(short, long, display_order = 3)]
    mask: Vec<String>,
}

#[derive(Args, Debug)]
#[command(group = ArgGroup::new("cloud").required(true).multiple(false))]
pub struct EnvConfig {
    #[cfg(feature = "aws")]
    #[command(flatten)]
    aws: AwsConfig,

    #[cfg(feature = "azure")]
    #[command(flatten)]
    azure: AzureConfig,

    #[cfg(feature = "google")]
    #[command(flatten)]
    google: GoogleConfig,

    #[cfg(feature = "vault")]
    #[command(flatten)]
    vault: HashicorpVaultConfig,

    #[command(flatten)]
    data: DataConfig,
}

impl EnvConfig {
    fn into_run_config(self) -> Result<(Box<dyn Vault>, DataConfig)> {
        #[cfg(feature = "aws")]
        if self.aws.is_enabled() {
            return Ok((Box::new(self.aws.into_vault()?), self.data));
        }

        #[cfg(feature = "azure")]
        if self.azure.is_enabled() {
            return Ok((Box::new(self.azure.into_vault()?), self.data));
        }

        #[cfg(feature = "google")]
        if self.google.is_enabled() {
            return Ok((Box::new(self.google.into_vault()?), self.data));
        }

        #[cfg(feature = "vault")]
        if self.vault.is_enabled() {
            return Ok((Box::new(self.vault.into_vault()?), self.data));
        }

        #[cfg(not(any(
            feature = "aws",
            feature = "azure",
            feature = "google",
            feature = "vault"
        )))]
        compile_error!("no cloud configured");

        unreachable!()
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
