use anyhow::Result;
use clap::{Clap, ValueHint};
use std::{fs, io, path::PathBuf};
use tempfile::NamedTempFile;
use thiserror::Error;

use crate::env;

#[derive(Error, Debug)]
pub enum CacheError {
    #[error("cannot load the environment")]
    LoadError(#[from] env::EnvLoadError),

    #[error("cannot store the resulting env file")]
    IoError(#[from] io::Error),

    #[error("cannot store the resulting env file - there was a problem during serialization")]
    SerializationError(#[from] serde_json::Error),
}

/// Caches the environment variables from KeyVault into local file.
#[derive(Clap, Debug)]
pub struct Cache {
    #[clap(flatten)]
    env: env::EnvConfig,

    #[clap(flatten)]
    output_file: OutputFileConfig,
}

#[derive(Clap, Debug)]
pub struct OutputFileConfig {
    /// The output file where cached configuration will be saved. Defaults to random temporary file
    /// if not specified.
    #[clap(short = 'f', long, parse(from_os_str), value_hint = ValueHint::FilePath, group = "output")]
    output_file: Option<PathBuf>,

    /// The output directory where cached configuration will be saved. If specified, a random file
    /// will be created there.
    #[clap(short = 'd', long, parse(from_os_str), value_hint = ValueHint::DirPath, group = "output")]
    output_dir: Option<PathBuf>,
}

enum OutputFile {
    Direct(fs::File, PathBuf),
    Temp(NamedTempFile),
}

fn get_output_file(cfg: OutputFileConfig) -> Result<OutputFile> {
    if let Some(f) = cfg.output_file {
        let file = fs::File::create(&f).map_err(CacheError::IoError)?;
        Ok(OutputFile::Direct(file, f))
    } else {
        let mut b = tempfile::Builder::new();
        b.prefix("kvenv-").suffix(".json").rand_bytes(5);
        let file = if let Some(d) = cfg.output_dir {
            b.tempfile_in(d)
        } else {
            b.tempfile()
        };
        let file = file.map_err(CacheError::IoError)?;
        Ok(OutputFile::Temp(file))
    }
}

fn store_env(e: env::ProcessEnv, out_file: OutputFile) -> Result<PathBuf> {
    match out_file {
        OutputFile::Direct(f, p) => {
            e.to_writer(f).map_err(CacheError::SerializationError)?;
            Ok(p)
        }
        OutputFile::Temp(mut t) => {
            e.to_writer(t.as_file_mut())
                .map_err(CacheError::SerializationError)?;
            let (_, p) = t.keep().map_err(|e| CacheError::IoError(e.error))?;
            Ok(p.as_path().to_owned())
        }
    }
}

pub fn run_cache(c: Cache) -> Result<()> {
    let cached_env = env::prepare_env(c.env).map_err(CacheError::LoadError)?;
    let out_file = get_output_file(c.output_file)?;
    let path = store_env(cached_env, out_file)?;
    println!("{}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::prelude::*;

    #[test]
    fn output_file_direct() {
        let cfg = OutputFileConfig {
            output_file: Some("./test-file.json".into()),
            output_dir: None,
        };
        assert_direct(cfg);

        let cfg = OutputFileConfig {
            output_file: Some("./test-file.json".into()),
            output_dir: Some("./should-be-ignored".into()),
        };
        assert_direct(cfg);
    }

    #[test]
    fn output_file_temp() {
        let cfg = OutputFileConfig {
            output_file: None,
            output_dir: None,
        };
        assert_temp(cfg);

        let cfg = OutputFileConfig {
            output_file: None,
            output_dir: Some(".".into()),
        };
        assert_temp(cfg);
    }

    fn assert_direct(cfg: OutputFileConfig) {
        let file_name = cfg.output_file.clone().unwrap();
        let f = get_output_file(cfg).unwrap();
        match f {
            OutputFile::Direct(mut f, _) => {
                write!(f, "test").unwrap(); // Try write
                drop(f);
                fs::remove_file(file_name).unwrap();
            }
            _ => panic!("should return `Direct` case"),
        };
    }

    fn assert_temp(cfg: OutputFileConfig) {
        let f = get_output_file(cfg).unwrap();
        match f {
            OutputFile::Temp(mut f) => {
                write!(f.as_file_mut(), "test").unwrap(); // Try write
                drop(f);
            }
            _ => panic!("should return `Temp` case"),
        };
    }
}
