use lxp_scan::features::{clones, context, drift, dupes, impact};
use lxp_scan::output::report;
use lxp_scan::scan::discover;
use std::path::Path;

pub fn report_warnings(warnings: &[String], verbose: bool) {
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

pub fn run_drift(root: &Path, json: bool, verbose: bool) -> anyhow::Result<()> {
    let mut warnings = Vec::new();
    let repos = discover::discover_repos(root, &mut warnings)?;
    report_warnings(&warnings, verbose);
    let rows = drift::compute_drift(&repos);
    if json {
        println!("{}", report::drift_json(&rows)?);
    } else {
        let names: Vec<String> = repos.iter().map(|r| r.name.clone()).collect();
        println!("{}", report::drift_table(&rows, &names));
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

pub fn run_impact(
    root: &Path,
    symbol: &str,
    from: Option<&str>,
    json: bool,
    verbose: bool,
) -> anyhow::Result<()> {
    let mut warnings = Vec::new();
    let hits = impact::run_impact(root, symbol, from, &mut warnings)?;
    report_warnings(&warnings, verbose);
    if json {
        println!("{}", report::impact_json(&hits)?);
    } else {
        if !hits.is_empty() {
            print!("{}", report::impact_report(&hits));
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

pub fn run_context(
    root: &Path,
    symbol: &str,
    from: Option<&str>,
    sites: usize,
    json: bool,
    verbose: bool,
) -> anyhow::Result<()> {
    let mut warnings = Vec::new();
    let pack = context::build_context(root, symbol, from, sites, &mut warnings)?;
    report_warnings(&warnings, verbose);
    if json {
        println!("{}", report::context_json(&pack)?);
    } else {
        print!(
            "{}",
            report::context_markdown(&pack, &root.display().to_string())
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

pub fn run_dupes(root: &Path, json: bool, verbose: bool) -> anyhow::Result<()> {
    let mut warnings = Vec::new();
    let groups = dupes::find_dupes(root, &mut warnings)?;
    report_warnings(&warnings, verbose);
    if json {
        println!("{}", report::dupes_json(&groups)?);
    } else {
        print!("{}", report::dupes_report(&groups));
        eprintln!(
            "\n{} name(s) exported from more than one repo",
            groups.len()
        );
    }
    Ok(())
}

pub fn run_clones(
    root: &Path,
    opts: clones::CloneOptions,
    json: bool,
    verbose: bool,
) -> anyhow::Result<()> {
    let mut warnings = Vec::new();
    let out = clones::find_clones(root, &opts, &mut warnings)?;
    report_warnings(&warnings, verbose);
    if json {
        println!("{}", report::clones_json(&out)?);
    } else {
        print!("{}", report::clones_report(&out));
        eprintln!("\n{} clone cluster(s)", out.clusters.len());
        if out.clusters.is_empty() {
            eprintln!(
                "hint: no clusters under {} — check --root, lower --min-tokens, or add --same-file/--verbose",
                root.display()
            );
        }
    }
    Ok(())
}
