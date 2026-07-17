use rayon::prelude::*;
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::Path;

use crate::scan::{analyzer, discover, resolver::RepoResolver, walker};

#[derive(Serialize, Debug)]
pub struct ImpactHit {
    pub repo: String,
    /// path relative to the repo root
    pub file: String,
    pub line: usize,
    /// resolved import source (package verbatim / repo-relative file path)
    pub source: String,
    pub refs: usize,
    pub jsx_uses: usize,
    pub jsx_props: BTreeSet<String>,
    /// 1-based line of each JSX render site (the import line is `line`)
    pub jsx_lines: Vec<usize>,
}

/// Scans every discovered repo under `root` for imports of `symbol`.
/// `from` is a substring filter on the resolved import source display
/// (package name verbatim, or repo-relative path for intra-repo files),
/// so alias and relative imports of the same module match the same filter.
pub fn run_impact(
    root: &Path,
    symbol: &str,
    from: Option<&str>,
    warnings: &mut Vec<String>,
) -> anyhow::Result<Vec<ImpactHit>> {
    let repos = discover::discover_repos(root, warnings)?;
    let mut hits = Vec::new();
    for repo in &repos {
        let resolver = RepoResolver::from_repo(&repo.root, warnings);
        let files = walker::source_files(&repo.root);
        // Per-file parallel scan; each file yields (hits, warnings) so
        // warnings from the parallel section reach the caller's vec.
        let (file_hits, file_warnings): (Vec<Vec<ImpactHit>>, Vec<Vec<String>>) = files
            .par_iter()
            .map(|path| scan_file(repo, &resolver, path, symbol, from))
            .unzip();
        hits.extend(file_hits.into_iter().flatten());
        warnings.extend(file_warnings.into_iter().flatten());
    }
    hits.sort_by(|a, b| (&a.repo, &a.file, a.line).cmp(&(&b.repo, &b.file, b.line)));
    Ok(hits)
}

fn scan_file(
    repo: &discover::Repo,
    resolver: &RepoResolver,
    path: &Path,
    symbol: &str,
    from: Option<&str>,
) -> (Vec<ImpactHit>, Vec<String>) {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            return (
                Vec::new(),
                vec![format!("{}: unreadable ({err}), skipped", path.display())],
            );
        }
    };
    // Prefilter: no textual occurrence means no import match — skip the parse.
    if !text.contains(symbol) {
        return (Vec::new(), Vec::new());
    }
    let findings = match analyzer::analyze_file(path, &text, symbol) {
        Ok(findings) => findings,
        Err(err) => return (Vec::new(), vec![format!("{err:#}, skipped")]),
    };
    let file = path
        .strip_prefix(&repo.root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned();
    let hits = findings
        .into_iter()
        .filter_map(|f| {
            let source = resolver.display(&resolver.resolve(path, &f.source));
            if let Some(filter) = from
                && !source.contains(filter)
            {
                return None;
            }
            Some(ImpactHit {
                repo: repo.name.clone(),
                file: file.clone(),
                line: f.line,
                source,
                refs: f.refs,
                jsx_uses: f.jsx_uses,
                jsx_props: f.jsx_props,
                jsx_lines: f.jsx_lines,
            })
        })
        .collect();
    (hits, Vec::new())
}
