use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "lxp-scan", version, about = "Cross-repo FE intelligence CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand)]
pub enum Cmd {
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
    /// Find name-agnostic structural clones: top-level functions with
    /// identical normalized bodies across repos, even under different names
    Clones {
        /// Minimum normalized body tokens for a candidate to participate
        #[arg(long, default_value_t = lxp_scan::scan::fingerprint::DEFAULT_MIN_TOKENS)]
        min_tokens: usize,
        /// Only report clusters containing this declaration name
        #[arg(long)]
        symbol: Option<String>,
        /// Declaration form to scan (a cluster can mix forms; this filters candidates)
        #[arg(long, value_enum, default_value_t = lxp_scan::features::clones::KindFilter::All)]
        kind: lxp_scan::features::clones::KindFilter,
        /// Also report clusters whose members all live in one file
        #[arg(long)]
        same_file: bool,
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
    fn clones_parses_flags_and_defaults_the_floor() {
        let cli = Cli::parse_from([
            "lxp-scan",
            "clones",
            "--symbol",
            "isEmail",
            "--kind",
            "const",
            "--same-file",
            "--json",
        ]);
        match cli.cmd {
            Cmd::Clones {
                min_tokens,
                symbol,
                kind,
                same_file,
                root,
                json,
                verbose,
            } => {
                assert_eq!(min_tokens, lxp_scan::scan::fingerprint::DEFAULT_MIN_TOKENS);
                assert_eq!(symbol.as_deref(), Some("isEmail"));
                assert_eq!(kind, lxp_scan::features::clones::KindFilter::Const);
                assert!(same_file);
                assert_eq!(root, PathBuf::from("."));
                assert!(json);
                assert!(!verbose);
            }
            _ => panic!("expected Clones subcommand"),
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
