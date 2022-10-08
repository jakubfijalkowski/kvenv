use anyhow::Result;
use clap::{arg, Args, ValueHint};
use std::{
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

use crate::env::ProcessEnv;
use crate::run;

#[derive(Error, Debug)]
pub enum RunWithError {
    #[error("cannot load environment file")]
    Load(#[from] serde_json::error::Error),
    #[error("cannot load environment file - io error")]
    Io(#[source] std::io::Error),
    #[error("cannot remove the env file")]
    Cleanup(#[source] std::io::Error),
    #[error("cannot run the specified command")]
    Run(#[source] anyhow::Error),
}

/// Runs the command with the specified argument using cached environment.
#[derive(Args, Debug)]
#[command(name = "run-with")]
pub struct RunWith {
    /// Path to the environment file created with `cache` command.
    #[arg(short, long, value_parser, value_hint = ValueHint::FilePath)]
    env_file: PathBuf,

    /// If set, the env file will be removed after execution.
    #[arg(short, long)]
    cleanup: bool,

    /// The command to execute
    #[arg(name = "COMMAND", required = true, last = true)]
    command: Vec<String>,
}

fn load_env(path: &Path) -> Result<ProcessEnv> {
    let file = fs::File::open(path).map_err(RunWithError::Io)?;
    let env = ProcessEnv::from_reader(&file).map_err(RunWithError::Load)?;
    Ok(env)
}

pub fn run_with(cfg: RunWith) -> Result<std::convert::Infallible> {
    let env = load_env(&cfg.env_file)?;

    let status =
        run::run_in_env(env, cfg.command).map_err(|x| anyhow::Error::new(RunWithError::Run(x)))?;
    if status.success() {
        if cfg.cleanup {
            fs::remove_file(&cfg.env_file).map_err(RunWithError::Cleanup)?;
        }

        std::process::exit(0)
    } else if let Some(code) = status.code() {
        std::process::exit(code)
    } else {
        std::process::exit(-1)
    }
}
