use anyhow::Result;
use clap::{AppSettings, Clap};

mod cache;
mod env;
mod run;
mod run_in;
mod run_with;

#[derive(Clap, Debug)]
#[clap(
    name = "kvenv",
    about,
    version,
    author,
    setting = AppSettings::ArgsNegateSubcommands,
    setting = AppSettings::DisableHelpSubcommand,
    setting = AppSettings::UnifiedHelpMessage,
    setting = AppSettings::AllowMissingPositional,
)]
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

#[cfg(test)]
mod tests {
    use super::Opts;
    use clap::{Clap, ErrorKind};

    #[test]
    fn can_clap_help() {
        assert_correct(&["kvenv", "cache", "--help"]);
        assert_correct(&["kvenv", "run-in", "--help"]);
        assert_correct(&["kvenv", "run-with", "--help"]);
    }

    fn assert_correct(args: &[&str]) {
        let opts = Opts::try_parse_from(args);
        let err = opts.unwrap_err();
        assert_eq!(ErrorKind::DisplayHelp, err.kind);
    }
}
