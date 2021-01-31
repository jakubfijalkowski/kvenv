use anyhow::Result;
use clap::Clap;

mod cache;
mod env;
mod run;
mod run_in;
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
    RunWith(run_with::RunWith),
    #[clap()]
    RunIn(run_in::RunIn),
}

fn main() -> Result<()> {
    let opts: Opts = Opts::parse();
    match opts.subcommand {
        SubCommand::Cache(c) => {
            cache::run_cache(c)?;
        }
        SubCommand::RunWith(c) => {
            run_with::run_with(c)?;
        }
        SubCommand::RunIn(c) => {
            run_in::run_in(c)?;
        }
    }
    Ok(())
}
