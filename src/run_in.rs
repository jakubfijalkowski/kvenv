use anyhow::Result;
use clap::{ArgSettings, Clap};
use thiserror::Error;

use crate::env::{download_env_sync, EnvConfig, EnvLoadError};
use crate::run;

#[derive(Error, Debug)]
pub enum RunInError {
    #[error("cannot load environment")]
    LoadError(#[from] EnvLoadError),
    #[error("cannot run the specified command")]
    RunError(#[source] anyhow::Error),
}

/// Runs the command with the specified argument using freshly downloaded environment.
#[derive(Clap, Debug)]
#[clap(name = "run-in")]
pub struct RunIn {
    #[clap(flatten)]
    env: EnvConfig,

    /// The command to execute
    #[clap(name = "COMMAND", required = true, setting = ArgSettings::Last)]
    command: Vec<String>,
}

pub fn run_in(cfg: RunIn) -> Result<std::convert::Infallible> {
    let env = download_env_sync(cfg.env).map_err(RunInError::LoadError)?;

    let status = run::run_in_env(env, cfg.command)
        .map_err(|x| anyhow::Error::new(RunInError::RunError(x)))?;
    if status.success() {
        std::process::exit(0)
    } else {
        if let Some(code) = status.code() {
            std::process::exit(code)
        } else {
            std::process::exit(-1)
        }
    }
}
