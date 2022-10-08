use anyhow::Result;
use clap::{command, Parser, Subcommand};

mod cache;
mod env;
mod run;
mod run_in;
mod run_with;

#[derive(Parser, Debug)]
#[command(name = "kvenv", about, version, author, next_line_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Cache(cache::Cache),
    RunWith(run_with::RunWith),
    RunIn(run_in::RunIn),
}

fn main() -> Result<()> {
    let opts: Cli = Cli::parse();
    match opts.command {
        Command::Cache(c) => {
            cache::run_cache(c)?;
        }
        Command::RunWith(c) => {
            run_with::run_with(c)?;
        }
        Command::RunIn(c) => {
            run_in::run_in(c)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::{error::ErrorKind, Parser};

    use super::Cli;

    #[test]
    fn can_clap_help() {
        assert_correct(&["kvenv", "cache", "--help"]);
        assert_correct(&["kvenv", "run-in", "--help"]);
        assert_correct(&["kvenv", "run-with", "--help"]);
    }

    fn assert_correct(args: &[&str]) {
        let opts = Cli::try_parse_from(args);
        let err = opts.unwrap_err();
        assert_eq!(ErrorKind::DisplayHelp, err.kind());
    }
}
