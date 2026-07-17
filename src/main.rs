mod cli;
mod commands;

use clap::Parser;
use cli::{Cli, Cmd};
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result: anyhow::Result<()> = match cli.cmd {
        Cmd::Impact {
            symbol,
            from,
            root,
            json,
            verbose,
        } => commands::run_impact(&root, &symbol, from.as_deref(), json, verbose),
        Cmd::Context {
            symbol,
            from,
            sites,
            root,
            json,
            verbose,
        } => commands::run_context(&root, &symbol, from.as_deref(), sites, json, verbose),
        Cmd::Mcp { root } => lxp_scan::mcp::serve(&root),
        Cmd::Tui { root } => lxp_scan::tui::run(&root),
        Cmd::Dupes {
            root,
            json,
            verbose,
        } => commands::run_dupes(&root, json, verbose),
        Cmd::Clones {
            min_tokens,
            symbol,
            kind,
            same_file,
            root,
            json,
            verbose,
        } => commands::run_clones(
            &root,
            lxp_scan::features::clones::CloneOptions {
                min_tokens,
                symbol,
                kind,
                same_file,
            },
            json,
            verbose,
        ),
        Cmd::Drift {
            root,
            json,
            verbose,
        } => commands::run_drift(&root, json, verbose),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}
