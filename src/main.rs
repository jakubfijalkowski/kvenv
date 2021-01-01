use anyhow::Result;
use clap::Clap;

mod cache;
mod env;

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
}

fn main() -> Result<()> {
    let opts: Opts = Opts::parse();
    match opts.subcommand {
        SubCommand::Cache(c) => {
            cache::run_cache(c)?;
        }
    }
    Ok(())
}
