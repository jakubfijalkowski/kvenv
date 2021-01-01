use anyhow::Result;
use clap::Clap;

mod cache;
mod cleanup;
mod env;
mod run;
mod run_with;

#[derive(Clap, Debug)]
#[clap(name = "kvenv", about, version, author)]
struct Opts {
    #[clap(subcommand)]
    subcommand: SubCommand,
}

#[derive(Clap, Debug)]
enum SubCommand {
    #[clap()]
    Cache(cache::Cache),
    #[clap()]
    Cleanup(cleanup::Cleanup),
    #[clap()]
    RunWith(run_with::RunWith),
}

fn main() -> Result<()> {
    let opts: Opts = Opts::parse();
    match opts.subcommand {
        SubCommand::Cache(c) => {
            cache::run_cache(c)?;
        }
        SubCommand::Cleanup(c) => {
            cleanup::run_cleanup(c)?;
        }
        SubCommand::RunWith(c) => {
            run_with::run_with(c)?;
        }
    }
    Ok(())
}
