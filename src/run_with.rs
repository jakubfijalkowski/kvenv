use anyhow::Result;
use clap::{ArgSettings, Clap, ValueHint};
use std::{fs, path::PathBuf};
use thiserror::Error;

use crate::env::ProcessEnv;
use crate::run;

#[derive(Error, Debug)]
pub enum RunWithError {
    #[error("cannot load environment file")]
    LoadError(#[from] serde_json::error::Error),
    #[error("cannot load environment file - io error")]
    IoError(#[source] std::io::Error),
    #[error("cannot remove the env file")]
    CleanupError(#[source] std::io::Error),
    #[error("cannot run the specified command")]
    RunError(#[source] anyhow::Error),
}

/// Runs the command with the specified argument using cached environment.
#[derive(Clap, Debug)]
#[clap(name = "run-with")]
pub struct RunWith {
    /// Path to the environment file created with `cache` command.
    #[clap(short, long, parse(from_os_str), value_hint = ValueHint::FilePath)]
    env_file: PathBuf,

    /// If set, the env file will be removed after execution.
    #[clap(short, long)]
    cleanup: bool,

    /// The command to execute
    #[clap(name = "COMMAND", required = true, setting = ArgSettings::Last)]
    command: Vec<String>,
}

fn load_env(path: &PathBuf) -> Result<ProcessEnv> {
    let file = fs::File::open(path).map_err(RunWithError::IoError)?;
    let env = ProcessEnv::from_reader(&file).map_err(RunWithError::LoadError)?;
    Ok(env)
}

pub fn run_with(cfg: RunWith) -> Result<std::convert::Infallible> {
    let env = load_env(&cfg.env_file)?;

    let status = run::run_in_env(env, cfg.command)
        .map_err(|x| anyhow::Error::new(RunWithError::RunError(x)))?;
    if status.success() {
        if cfg.cleanup {
            fs::remove_file(&cfg.env_file).map_err(RunWithError::CleanupError)?;
        }

        std::process::exit(0)
    } else if let Some(code) = status.code() {
        std::process::exit(code)
    } else {
        std::process::exit(-1)
    }
}
