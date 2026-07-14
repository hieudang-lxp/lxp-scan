use serde::Serialize;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::Path;

use crate::definition::{Definition, find_definition};
use crate::discover::{Repo, discover_repos};
use crate::impact::{ImpactHit, run_impact};

/// Lines shown per usage excerpt: the anchor line plus enough after it to
/// cover a typical multi-line JSX render.
const EXCERPT_LINES: usize = 8;

#[derive(Serialize, Debug)]
pub struct UsageExcerpt {
    pub repo: String,
    pub file: String,
    /// anchor line: first JSX render site, or the import line for
    /// ref-only/import-only hits
    pub line: usize,
    pub jsx_props: BTreeSet<String>,
    pub code: String,
}

#[derive(Serialize, Debug)]
pub struct ContextPack {
    pub symbol: String,
    pub total_sites: usize,
    pub total_files: usize,
    pub total_repos: usize,
    /// (prop, number of sites passing it), most frequent first
    pub prop_counts: Vec<(String, usize)>,
    pub definition: Option<Definition>,
    pub excerpts: Vec<UsageExcerpt>,
}

/// Builds the LLM-ready context pack: full impact scan, prop frequencies,
/// definition lookup, and up to `sites` representative usage excerpts.
pub fn build_context(
    root: &Path,
    symbol: &str,
    from: Option<&str>,
    sites: usize,
    warnings: &mut Vec<String>,
) -> anyhow::Result<ContextPack> {
    let repos = discover_repos(root, warnings)?;
    let hits = run_impact(root, symbol, from, warnings)?;

    let mut prop_counts: HashMap<&String, usize> = HashMap::new();
    for hit in &hits {
        for prop in &hit.jsx_props {
            *prop_counts.entry(prop).or_default() += 1;
        }
    }
    let mut prop_counts: Vec<(String, usize)> = prop_counts
        .into_iter()
        .map(|(prop, count)| (prop.clone(), count))
        .collect();
    prop_counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    let total_files = hits
        .iter()
        .map(|h| (h.repo.as_str(), h.file.as_str()))
        .collect::<BTreeSet<_>>()
        .len();
    let total_repos = hits
        .iter()
        .map(|h| h.repo.as_str())
        .collect::<BTreeSet<_>>()
        .len();

    let definition = find_definition(symbol, &repos, &hits, warnings);
    let excerpts = select_sites(&hits, sites)
        .into_iter()
        .filter_map(|hit| read_excerpt(&repos, hit, warnings))
        .collect();

    Ok(ContextPack {
        symbol: symbol.to_string(),
        total_sites: hits.len(),
        total_files,
        total_repos,
        prop_counts,
        definition,
        excerpts,
    })
}

/// Representative selection: round-robin across repos so no repo dominates;
/// within a repo JSX renders come before ref-only hits, and a hit whose prop
/// set hasn't been shown yet is preferred over repeating a seen combination.
fn select_sites(hits: &[ImpactHit], sites: usize) -> Vec<&ImpactHit> {
    let mut queues: Vec<VecDeque<&ImpactHit>> = Vec::new();
    let mut by_repo: HashMap<&str, usize> = HashMap::new();
    for hit in hits {
        let idx = *by_repo.entry(hit.repo.as_str()).or_insert_with(|| {
            queues.push(VecDeque::new());
            queues.len() - 1
        });
        queues[idx].push_back(hit);
    }
    for queue in &mut queues {
        // stable: keeps (file, line) order within each preference class;
        // test files are noise in a context pack, so they go last
        queue
            .make_contiguous()
            .sort_by_key(|h| (is_test_file(&h.file), if h.jsx_uses > 0 { 0 } else { 1 }));
    }

    let mut seen_props: HashSet<String> = HashSet::new();
    let mut selected = Vec::new();
    while selected.len() < sites && queues.iter().any(|q| !q.is_empty()) {
        for queue in &mut queues {
            if selected.len() == sites {
                break;
            }
            let Some(idx) = queue
                .iter()
                .position(|h| {
                    !is_test_file(&h.file)
                        && h.jsx_uses > 0
                        && !seen_props.contains(&props_key(h))
                })
                .or(if queue.is_empty() { None } else { Some(0) })
            else {
                continue;
            };
            let hit = queue.remove(idx).expect("index from position/non-empty");
            seen_props.insert(props_key(hit));
            selected.push(hit);
        }
    }
    selected
}

fn is_test_file(file: &str) -> bool {
    file.contains("__test__")
        || file.contains("__tests__")
        || file.contains(".test.")
        || file.contains(".spec.")
}

fn props_key(hit: &ImpactHit) -> String {
    hit.jsx_props
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(",")
}

fn read_excerpt(
    repos: &[Repo],
    hit: &ImpactHit,
    warnings: &mut Vec<String>,
) -> Option<UsageExcerpt> {
    let repo = repos.iter().find(|r| r.name == hit.repo)?;
    let path = repo.root.join(&hit.file);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) => {
            warnings.push(format!("{}: unreadable ({err}), excerpt skipped", path.display()));
            return None;
        }
    };
    let anchor = hit.jsx_lines.first().copied().unwrap_or(hit.line);
    let code = text
        .lines()
        .skip(anchor - 1)
        .take(EXCERPT_LINES)
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string();
    Some(UsageExcerpt {
        repo: hit.repo.clone(),
        file: hit.file.clone(),
        line: anchor,
        jsx_props: hit.jsx_props.clone(),
        code,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn workspace() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/workspace")
    }

    #[test]
    fn pack_carries_totals_definition_and_prop_frequencies() {
        let mut warnings = Vec::new();
        let pack = build_context(&workspace(), "Button", None, 8, &mut warnings).unwrap();
        assert_eq!(pack.symbol, "Button");
        assert!(pack.total_sites >= 2);
        assert_eq!(pack.total_repos, 2);
        let def = pack.definition.expect("fake-lib definition");
        assert_eq!(def.repo, "fake-lib");
        assert!(pack.prop_counts.iter().any(|(p, n)| p == "variant" && *n >= 1));
    }

    #[test]
    fn excerpts_anchor_on_the_render_site_not_the_import() {
        let mut warnings = Vec::new();
        let pack = build_context(&workspace(), "Button", None, 8, &mut warnings).unwrap();
        let page = pack
            .excerpts
            .iter()
            .find(|e| e.file.ends_with("page.tsx"))
            .expect("page.tsx excerpt");
        assert!(page.code.starts_with("  <Button"), "got: {}", page.code);
        assert!(page.code.contains("variant=\"primary\""));
        assert!(page.line > 1, "anchor must be the JSX line, not the import");
    }

    fn hit(repo: &str, file: &str, jsx_uses: usize) -> ImpactHit {
        ImpactHit {
            repo: repo.to_string(),
            file: file.to_string(),
            line: 1,
            source: "lib".to_string(),
            refs: 0,
            jsx_uses,
            jsx_props: Default::default(),
            jsx_lines: if jsx_uses > 0 { vec![1] } else { vec![] },
        }
    }

    #[test]
    fn test_files_are_selected_last() {
        let hits = vec![
            hit("app", "src/__test__/Thing.test.tsx", 1),
            hit("app", "src/Thing.spec.tsx", 1),
            hit("app", "src/pages/Real.tsx", 1),
        ];
        let selected = select_sites(&hits, 1);
        assert_eq!(selected[0].file, "src/pages/Real.tsx");
        // test files still appear once real sites are exhausted
        let all = select_sites(&hits, 3);
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn selection_round_robins_across_repos_and_respects_cap() {
        let mut warnings = Vec::new();
        let pack = build_context(&workspace(), "Button", None, 2, &mut warnings).unwrap();
        assert_eq!(pack.excerpts.len(), 2);
        let repos: BTreeSet<&str> = pack.excerpts.iter().map(|e| e.repo.as_str()).collect();
        assert_eq!(repos.len(), 2, "one excerpt per repo before repeats");
    }
}
