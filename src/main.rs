use clap::{Clap, ValueHint};
use std::path::PathBuf;
use anyhow::Result;

mod env;

use env::{prepare_env, EnvConfig};

#[derive(Clap, Debug)]
#[clap(name = "kvenv", about, version, author)]
struct Opts {
    #[clap(subcommand)]
    subcommand: SubCommand,
}

#[derive(Clap, Debug)]
enum SubCommand {
    #[clap()]
    Cache(Cache),
}

#[derive(Clap, Debug)]
struct Cache {
    #[clap(flatten)]
    env: EnvConfig,

    /// The output file where cached configuration will be saved. Defaults to random temporary file
    /// if not specified.
    #[clap(short, long, parse(from_os_str), value_hint = ValueHint::FilePath)]
    output: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let opts: Opts = Opts::parse();
    match opts.subcommand {
        SubCommand::Cache(c) => {
            let env = prepare_env(c.env).await?;
            println!("{:?}", env);
        }
    }
    Ok(())
}
