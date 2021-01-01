use anyhow::{Error, Result};
use clap::{ArgSettings, Clap, ValueHint};
use std::process::{Command, ExitStatus};
use std::{fs, path::PathBuf};
use thiserror::Error;

use crate::env::ProcessEnv;

#[derive(Error, Debug)]
pub enum RunWithError {
    #[error("cannot load environment file")]
    LoadError(#[from] serde_json::error::Error),
    #[error("cannot load environment file - io error")]
    IoError(#[source] std::io::Error),
    #[error("cannot remove the env file")]
    CleanupError(#[source] std::io::Error),
    #[error("cannot run the specified command")]
    RunError(#[source] std::io::Error),
    #[error("the program failed with unknown exit code: {0}")]
    Failed(ExitStatus),
}

/// Runs the command with the specified argument.
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

pub fn run_with(cfg: RunWith) -> Result<()> {
    let env = load_env(&cfg.env_file)?.to_env();

    let mut child = Command::new(&cfg.command[0])
        .args(cfg.command.iter().skip(1))
        .env_clear()
        .envs(&env)
        .spawn()
        .map_err(RunWithError::RunError)?;
    let result = child.wait().map_err(RunWithError::RunError)?;

    if result.success() && cfg.cleanup {
        fs::remove_file(&cfg.env_file).map_err(RunWithError::CleanupError)?;
    }

    if !result.success() {
        if let Some(c) = result.code() {
            std::process::exit(c);
        } else {
            Err(Error::new(RunWithError::Failed(result)))
        }
    } else {
        Ok(())
    }
}
