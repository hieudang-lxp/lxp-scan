use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "lxp-scan", version, about = "Cross-repo FE intelligence CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Find usage sites of a symbol across all repos
    Impact {
        symbol: String,
        /// Substring filter on the resolved import source
        #[arg(long)]
        from: Option<String>,
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        verbose: bool,
    },
    /// Show lxp-common-* / lxp-design-system version drift across repos
    Drift {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        verbose: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result: anyhow::Result<()> = match cli.cmd {
        Cmd::Impact { .. } => {
            eprintln!("impact: not implemented yet");
            Ok(())
        }
        Cmd::Drift { .. } => {
            eprintln!("drift: not implemented yet");
            Ok(())
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn impact_parses_symbol_and_flags() {
        let cli = Cli::parse_from([
            "lxp-scan", "impact", "Button", "--from", "lxp-design-system", "--root", "/repos",
            "--json", "--verbose",
        ]);
        match cli.cmd {
            Cmd::Impact {
                symbol,
                from,
                root,
                json,
                verbose,
            } => {
                assert_eq!(symbol, "Button");
                assert_eq!(from.as_deref(), Some("lxp-design-system"));
                assert_eq!(root, PathBuf::from("/repos"));
                assert!(json);
                assert!(verbose);
            }
            _ => panic!("expected Impact subcommand"),
        }
    }

    #[test]
    fn drift_defaults_root_to_current_dir() {
        let cli = Cli::parse_from(["lxp-scan", "drift"]);
        match cli.cmd {
            Cmd::Drift {
                root,
                json,
                verbose,
            } => {
                assert_eq!(root, PathBuf::from("."));
                assert!(!json);
                assert!(!verbose);
            }
            _ => panic!("expected Drift subcommand"),
        }
    }
}
