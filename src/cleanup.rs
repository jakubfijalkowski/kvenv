use anyhow::Result;
use clap::{Clap, ValueHint};
use std::{fs, path::PathBuf};

/// Cleanup leftovers from the `cache` command. Basically `rm`.
#[derive(Clap, Debug)]
pub struct Cleanup {
    /// The file to delete.
    #[clap(name = "FILE", parse(from_os_str), value_hint = ValueHint::FilePath)]
    file: PathBuf,
}

pub fn run_cleanup(cfg: Cleanup) -> Result<()> {
    fs::remove_file(cfg.file)?;
    Ok(())
}
