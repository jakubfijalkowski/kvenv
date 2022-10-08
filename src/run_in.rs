use anyhow::Result;
use clap::{arg, Args};
use thiserror::Error;

use crate::env::{download_env, EnvConfig};
use crate::run;

#[derive(Error, Debug)]
pub enum RunInError {
    #[error("cannot load environment")]
    LoadError(#[source] anyhow::Error),
    #[error("cannot run the specified command")]
    RunError(#[source] anyhow::Error),
}

/// Runs the command with the specified argument using freshly downloaded environment.
#[derive(Args, Debug)]
pub struct RunIn {
    #[command(flatten)]
    env: EnvConfig,

    /// The command to execute
    #[arg(name = "COMMAND", required = true)]
    command: Vec<String>,
}

pub fn run_in(cfg: RunIn) -> Result<std::convert::Infallible> {
    let env = download_env(cfg.env).map_err(RunInError::LoadError)?;

    let status = run::run_in_env(env, cfg.command)
        .map_err(|x| anyhow::Error::new(RunInError::RunError(x)))?;
    if status.success() {
        std::process::exit(0)
    } else if let Some(code) = status.code() {
        std::process::exit(code)
    } else {
        std::process::exit(-1)
    }
}
