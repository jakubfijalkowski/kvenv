use anyhow::Result;
use clap::{Clap, ValueHint};
use std::{fs, io, path::PathBuf};
use tempfile::NamedTempFile;
use thiserror::Error;

use crate::env;

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

#[derive(Error, Debug)]
pub enum CacheError {
    #[error("cannot load the environment")]
    LoadError(#[from] env::EnvLoadError),
    #[error("cannot store the resulting env file")]
    StoreError(#[from] io::Error),
}

enum OutputFile {
    Direct(fs::File),
    Temp(NamedTempFile),
}

fn get_output_file(cfg: &OutputFileConfig) -> Result<OutputFile> {
    if let Some(f) = &cfg.output_file {
        let file = fs::File::create(f).map_err(CacheError::StoreError)?;
        Ok(OutputFile::Direct(file))
    } else {
        let mut b = tempfile::Builder::new();
        b.prefix("kvenv-").suffix(".json").rand_bytes(5);
        let file = if let Some(d) = &cfg.output_dir {
            b.tempfile_in(d)
        } else {
            b.tempfile()
        };
        let file = file.map_err(CacheError::StoreError)?;
        Ok(OutputFile::Temp(file))
    }
}

pub fn run_cache(c: Cache) -> Result<()> {
    todo!()
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
        let f = get_output_file(&cfg).unwrap();
        assert!(matches!(f, OutputFile::Direct(_)));
        match f {
            OutputFile::Direct(mut f) => {
                write!(f, "test").unwrap(); // Try write
                drop(f);
                fs::remove_file(cfg.output_file.unwrap()).unwrap();
            }
            _ => unreachable!(),
        };
    }

    fn assert_temp(cfg: OutputFileConfig) {
        let f = get_output_file(&cfg).unwrap();
        assert!(matches!(f, OutputFile::Temp(_)));
        match f {
            OutputFile::Temp(mut f) => {
                write!(f.as_file_mut(), "test").unwrap(); // Try write
                drop(f);
            }
            _ => unreachable!(),
        };
    }
}
