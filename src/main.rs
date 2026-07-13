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
        Cmd::Impact {
            symbol,
            from,
            root,
            json,
            verbose,
        } => run_impact(&root, &symbol, from.as_deref(), json, verbose),
        Cmd::Drift {
            root,
            json,
            verbose,
        } => run_drift(&root, json, verbose),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn report_warnings(warnings: &[String], verbose: bool) {
    if verbose {
        for w in warnings {
            eprintln!("warn: {w}");
        }
    } else if !warnings.is_empty() {
        eprintln!(
            "{} warning(s) suppressed; rerun with --verbose",
            warnings.len()
        );
    }
}

fn run_drift(root: &std::path::Path, json: bool, verbose: bool) -> anyhow::Result<()> {
    let mut warnings = Vec::new();
    let repos = lxp_scan::discover::discover_repos(root, &mut warnings)?;
    report_warnings(&warnings, verbose);
    let rows = lxp_scan::drift::compute_drift(&repos);
    if json {
        println!("{}", lxp_scan::report::drift_json(&rows)?);
    } else {
        let names: Vec<String> = repos.iter().map(|r| r.name.clone()).collect();
        println!("{}", lxp_scan::report::drift_table(&rows, &names));
    }
    if rows.is_empty() {
        eprintln!(
            "no lxp-common-*/lxp-design-system dependencies found under {} ({} repo(s) discovered) — is --root pointing at the FE workspace?",
            root.display(),
            repos.len()
        );
    }
    Ok(())
}

fn run_impact(
    root: &std::path::Path,
    symbol: &str,
    from: Option<&str>,
    json: bool,
    verbose: bool,
) -> anyhow::Result<()> {
    let mut warnings = Vec::new();
    let hits = lxp_scan::impact::run_impact(root, symbol, from, &mut warnings)?;
    report_warnings(&warnings, verbose);
    if json {
        println!("{}", lxp_scan::report::impact_json(&hits)?);
    } else {
        println!("{}", lxp_scan::report::impact_table(&hits));
        let files: std::collections::BTreeSet<(&str, &str)> = hits
            .iter()
            .map(|h| (h.repo.as_str(), h.file.as_str()))
            .collect();
        eprintln!("{} usage site(s) in {} file(s)", hits.len(), files.len());
        if hits.is_empty() {
            eprintln!(
                "hint: no matches under {} — check --root, drop/adjust --from, or add --verbose",
                root.display()
            );
        }
    }
    Ok(())
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
            "lxp-scan",
            "impact",
            "Button",
            "--from",
            "lxp-design-system",
            "--root",
            "/repos",
            "--json",
            "--verbose",
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
