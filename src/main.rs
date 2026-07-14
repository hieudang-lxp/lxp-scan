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
    /// Emit an LLM-ready context pack for a symbol (definition + usage excerpts)
    Context {
        symbol: String,
        /// Substring filter on the resolved import source
        #[arg(long)]
        from: Option<String>,
        /// Maximum number of usage excerpts
        #[arg(long, default_value_t = 8)]
        sites: usize,
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        verbose: bool,
    },
    /// Run as an MCP stdio server exposing impact/context/drift/dupes to agents
    Mcp {
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Find same-name exported components declared in more than one repo
    Dupes {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        verbose: bool,
    },
    /// Interactive component explorer: fuzzy-find a symbol, browse its
    /// usages/props/definition, Enter opens the site in your editor
    Tui {
        #[arg(long, default_value = ".")]
        root: PathBuf,
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
        Cmd::Context {
            symbol,
            from,
            sites,
            root,
            json,
            verbose,
        } => run_context(&root, &symbol, from.as_deref(), sites, json, verbose),
        Cmd::Mcp { root } => lxp_scan::mcp::serve(&root),
        Cmd::Tui { root } => lxp_scan::tui::run(&root),
        Cmd::Dupes {
            root,
            json,
            verbose,
        } => run_dupes(&root, json, verbose),
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
        if !hits.is_empty() {
            print!("{}", lxp_scan::report::impact_report(&hits));
        }
        let files: std::collections::BTreeSet<(&str, &str)> = hits
            .iter()
            .map(|h| (h.repo.as_str(), h.file.as_str()))
            .collect();
        let repos: std::collections::BTreeSet<&str> =
            hits.iter().map(|h| h.repo.as_str()).collect();
        eprintln!(
            "\n{} usage site(s) in {} file(s) across {} repo(s)",
            hits.len(),
            files.len(),
            repos.len()
        );
        if hits.is_empty() {
            eprintln!(
                "hint: no matches under {} — check --root, drop/adjust --from, or add --verbose",
                root.display()
            );
        }
    }
    Ok(())
}

fn run_context(
    root: &std::path::Path,
    symbol: &str,
    from: Option<&str>,
    sites: usize,
    json: bool,
    verbose: bool,
) -> anyhow::Result<()> {
    let mut warnings = Vec::new();
    let pack = lxp_scan::context::build_context(root, symbol, from, sites, &mut warnings)?;
    report_warnings(&warnings, verbose);
    if json {
        println!("{}", lxp_scan::report::context_json(&pack)?);
    } else {
        print!(
            "{}",
            lxp_scan::report::context_markdown(&pack, &root.display().to_string())
        );
        if pack.total_sites == 0 {
            eprintln!(
                "hint: no matches under {} — check --root, drop/adjust --from, or add --verbose",
                root.display()
            );
        }
    }
    Ok(())
}

fn run_dupes(root: &std::path::Path, json: bool, verbose: bool) -> anyhow::Result<()> {
    let mut warnings = Vec::new();
    let groups = lxp_scan::dupes::find_dupes(root, &mut warnings)?;
    report_warnings(&warnings, verbose);
    if json {
        println!("{}", lxp_scan::report::dupes_json(&groups)?);
    } else {
        print!("{}", lxp_scan::report::dupes_report(&groups));
        eprintln!(
            "\n{} name(s) exported from more than one repo",
            groups.len()
        );
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
