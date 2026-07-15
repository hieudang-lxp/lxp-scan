use rayon::prelude::*;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::discover::Repo;
use crate::fingerprint::{self, Candidate, CandidateKind};
use crate::{analyzer, discover, walker};

/// Candidate-level kind filter (`--kind`). A cluster may mix declaration
/// forms (`const` arrow in one repo, `function` in another), so filtering
/// happens BEFORE clustering: `fn` scans only function declarations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum KindFilter {
    Fn,
    Const,
    #[default]
    All,
}

#[derive(Debug, Clone)]
pub struct CloneOptions {
    /// minimum normalized body tokens for a candidate to participate
    pub min_tokens: usize,
    /// keep only clusters containing this exact declaration name
    pub symbol: Option<String>,
    pub kind: KindFilter,
    /// also report clusters whose members all live in one file
    pub same_file: bool,
}

impl Default for CloneOptions {
    fn default() -> Self {
        Self {
            min_tokens: fingerprint::DEFAULT_MIN_TOKENS,
            symbol: None,
            kind: KindFilter::All,
            same_file: false,
        }
    }
}

#[derive(Serialize, Debug)]
pub struct CloneSite {
    pub repo: String,
    /// path relative to the repo root
    pub file: String,
    pub line: usize,
    pub name: String,
    pub exported: bool,
    pub kind: CandidateKind,
    pub sig: String,
}

#[derive(Serialize, Debug)]
pub struct CloneCluster {
    pub members: Vec<CloneSite>,
    /// normalized body token count (identical across members by construction)
    pub token_count: usize,
    /// display signature of the first member
    pub sig: String,
    /// string/regex literals shared by the cluster body, deduped
    pub literals: Vec<String>,
    /// npm cross-check notes (existing export in an lxp-common package, or
    /// candidate-shared-home hint)
    pub notes: Vec<String>,
}

#[derive(Serialize, Debug)]
pub struct ClonesOutput {
    pub clusters: Vec<CloneCluster>,
    /// lxp-common-* dependencies with no cloned source repo under the scan
    /// root — body-clone detection is blind to them (npm-only)
    pub npm_only_packages: Vec<String>,
}

/// Name-agnostic structural clone detection: fingerprints every top-level
/// function-shaped declaration and clusters identical normalized bodies
/// across (by default) different files. Complementary to `dupes`, which
/// matches exported component NAMES only.
pub fn find_clones(
    root: &Path,
    opts: &CloneOptions,
    warnings: &mut Vec<String>,
) -> anyhow::Result<ClonesOutput> {
    let repos = discover::discover_repos(root, warnings)?;

    // fingerprint: cluster payload carried alongside each site
    let mut by_fingerprint: BTreeMap<String, Vec<(CloneSite, usize, Vec<String>)>> =
        BTreeMap::new();
    for repo in &repos {
        let files = walker::source_files(&repo.root);
        let scanned: Vec<Result<Vec<(Candidate, String)>, String>> = files
            .par_iter()
            .filter_map(|path| {
                let name = path.to_string_lossy();
                // stories/tests copy implementations to exercise them — noise
                if name.contains(".stories.")
                    || name.contains(".test.")
                    || name.contains(".spec.")
                    || name.contains("__test__")
                    || name.contains("__tests__")
                {
                    return None;
                }
                let text = std::fs::read_to_string(path).ok()?;
                // prefilter: a file with no function syntax has no candidates
                if !text.contains("=>") && !text.contains("function") {
                    return None;
                }
                let file = path
                    .strip_prefix(&repo.root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .into_owned();
                match fingerprint::file_candidates(path, &text) {
                    Ok(cands) => Some(Ok(cands
                        .into_iter()
                        .filter(|c| c.token_count >= opts.min_tokens && kind_matches(opts.kind, c))
                        .map(|c| (c, file.clone()))
                        .collect())),
                    Err(err) => Some(Err(format!("{err:#}, skipped"))),
                }
            })
            .collect();
        for item in scanned {
            match item {
                Ok(candidates) => {
                    for (c, file) in candidates {
                        by_fingerprint.entry(c.fingerprint).or_default().push((
                            CloneSite {
                                repo: repo.name.clone(),
                                file,
                                line: c.line,
                                name: c.name,
                                exported: c.exported,
                                kind: c.kind,
                                sig: c.sig,
                            },
                            c.token_count,
                            c.literals,
                        ));
                    }
                }
                Err(warning) => warnings.push(warning),
            }
        }
    }

    let npm_only_packages = npm_only_common_packages(&repos);
    let npm_exports = common_package_exports(&repos, &npm_only_packages, warnings);

    let mut clusters: Vec<CloneCluster> = by_fingerprint
        .into_values()
        .filter_map(|mut members| {
            if members.len() < 2 {
                return None;
            }
            members.sort_by(|a, b| {
                (&a.0.repo, &a.0.file, a.0.line).cmp(&(&b.0.repo, &b.0.file, b.0.line))
            });
            let files: BTreeSet<(&str, &str)> = members
                .iter()
                .map(|(s, ..)| (s.repo.as_str(), s.file.as_str()))
                .collect();
            if files.len() < 2 && !opts.same_file {
                return None;
            }
            if let Some(symbol) = &opts.symbol
                && !members.iter().any(|(s, ..)| &s.name == symbol)
            {
                return None;
            }
            let token_count = members[0].1;
            let sig = members[0].0.sig.clone();
            let literals: Vec<String> = members[0]
                .2
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let sites: Vec<CloneSite> = members.into_iter().map(|(s, ..)| s).collect();
            let notes = npm_notes(&sites, &npm_exports);
            Some(CloneCluster {
                members: sites,
                token_count,
                sig,
                literals,
                notes,
            })
        })
        .collect();
    clusters.sort_by(|a, b| {
        b.members
            .len()
            .cmp(&a.members.len())
            .then_with(|| key_of(a).cmp(&key_of(b)))
    });

    Ok(ClonesOutput {
        clusters,
        npm_only_packages,
    })
}

fn key_of(cluster: &CloneCluster) -> (&str, &str, usize) {
    let first = &cluster.members[0];
    (&first.repo, &first.file, first.line)
}

fn kind_matches(filter: KindFilter, candidate: &Candidate) -> bool {
    match filter {
        KindFilter::All => true,
        KindFilter::Fn => candidate.kind == CandidateKind::Fn,
        KindFilter::Const => candidate.kind == CandidateKind::Const,
    }
}

/// lxp-common-* dependencies declared by any repo but not present as a cloned
/// source repo under the scan root — the scan is blind to their bodies.
fn npm_only_common_packages(repos: &[Repo]) -> Vec<String> {
    let repo_names: BTreeSet<&str> = repos.iter().map(|r| r.name.as_str()).collect();
    let mut out = BTreeSet::new();
    for repo in repos {
        for dep in repo.deps.keys() {
            if dep.starts_with("lxp-common-") && !repo_names.contains(dep.as_str()) {
                out.insert(dep.clone());
            }
        }
    }
    out.into_iter().collect()
}

/// Name-level cross-check against the compiled npm packages: exported names
/// harvested from `node_modules/<pkg>/**/*.d.ts`. Body comparison is
/// impossible on compiled output — this only answers "does an export with
/// this name already exist in the natural shared home?".
fn common_package_exports(
    repos: &[Repo],
    packages: &[String],
    warnings: &mut Vec<String>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for repo in repos {
        for pkg in packages {
            let dir = repo.root.join("node_modules").join(pkg);
            if !dir.is_dir() {
                continue;
            }
            let mut dts_files = Vec::new();
            collect_dts(&dir, &mut dts_files);
            for path in dts_files {
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                match analyzer::exported_values(&path, &text) {
                    Ok(exports) => {
                        let names = out.entry(pkg.clone()).or_default();
                        names.extend(exports.into_iter().map(|(name, _)| name));
                    }
                    Err(err) => warnings.push(format!("{err:#}, skipped")),
                }
            }
        }
    }
    out
}

/// The main walker deliberately excludes `.d.ts` and `node_modules`; this
/// dedicated scan targets exactly those, bounded to one package directory.
fn collect_dts(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if entry.file_name() != "node_modules" {
                collect_dts(&path, out);
            }
        } else if path.to_string_lossy().ends_with(".d.ts") {
            out.push(path);
        }
    }
    out.sort();
}

fn npm_notes(
    members: &[CloneSite],
    npm_exports: &BTreeMap<String, BTreeSet<String>>,
) -> Vec<String> {
    let mut notes = Vec::new();
    let names: BTreeSet<&str> = members.iter().map(|m| m.name.as_str()).collect();
    // a member living in a cloned lxp-common-* repo IS the shared home —
    // the fix is importing it, not creating one
    let common_repos: BTreeSet<&str> = members
        .iter()
        .filter(|m| m.repo.starts_with("lxp-common-"))
        .map(|m| m.repo.as_str())
        .collect();
    for repo in &common_repos {
        notes.push(format!(
            "already implemented in {repo} — import it instead of duplicating"
        ));
    }
    for (pkg, exports) in npm_exports {
        for name in &names {
            if exports.contains(*name) {
                notes.push(format!("an export named {name} already exists in {pkg}"));
            }
        }
    }
    if notes.is_empty() && !npm_exports.is_empty() {
        let names: Vec<&str> = names.into_iter().collect();
        let pkgs: Vec<&str> = npm_exports.keys().map(String::as_str).collect();
        notes.push(format!(
            "no {} export found in {} — candidate shared home",
            names.join("/"),
            pkgs.join(", ")
        ));
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn workspace() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/workspace")
    }

    fn scan(opts: &CloneOptions) -> (ClonesOutput, Vec<String>) {
        let mut warnings = Vec::new();
        let out = find_clones(&workspace(), opts, &mut warnings).unwrap();
        (out, warnings)
    }

    fn cluster_with<'a>(out: &'a ClonesOutput, name: &str) -> Option<&'a CloneCluster> {
        out.clusters
            .iter()
            .find(|c| c.members.iter().any(|m| m.name == name))
    }

    #[test]
    fn clean_email_pair_clusters_across_repos_despite_different_names() {
        let (out, _) = scan(&CloneOptions::default());
        let cluster = cluster_with(&out, "isEmail").expect("email cluster must exist");
        let names: Vec<&str> = cluster.members.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["isEmail", "validateEmail"], "sorted by repo");
        assert!(cluster.members.iter().all(|m| m.exported));
        assert_eq!(cluster.members[0].repo, "app-one");
        assert_eq!(cluster.members[1].repo, "app-two");
        assert!(cluster.members[0].file.ends_with("validators.ts"));
        assert!(
            cluster.literals.iter().any(|l| l.contains("[^\\s@]+@")),
            "the discriminating regex is surfaced: {:?}",
            cluster.literals
        );
    }

    #[test]
    fn same_shape_different_regex_does_not_join_the_email_cluster() {
        let (out, _) = scan(&CloneOptions::default());
        let email = cluster_with(&out, "isEmail").unwrap();
        assert!(
            !email.members.iter().any(|m| m.name == "isPhone"),
            "phone validator has a different regex literal"
        );
    }

    #[test]
    fn passthroughs_and_substring_names_never_cluster() {
        let (out, _) = scan(&CloneOptions::default());
        for name in ["identity", "alwaysTrue", "isEmailFlow"] {
            assert!(
                cluster_with(&out, name).is_none(),
                "{name} must not appear in any cluster"
            );
        }
    }

    #[test]
    fn raising_the_floor_drops_the_email_cluster() {
        let (out, _) = scan(&CloneOptions {
            min_tokens: 500,
            ..Default::default()
        });
        assert!(cluster_with(&out, "isEmail").is_none());
    }

    #[test]
    fn non_exported_copies_cluster_and_are_flagged_unexported() {
        let (out, _) = scan(&CloneOptions::default());
        let cluster = cluster_with(&out, "collapseWhitespace").expect("local util pair");
        assert_eq!(cluster.members.len(), 2);
        assert!(cluster.members.iter().all(|m| !m.exported));
    }

    #[test]
    fn same_file_clusters_are_hidden_by_default_and_shown_with_the_flag() {
        let (default_out, _) = scan(&CloneOptions::default());
        assert!(
            cluster_with(&default_out, "isPhone").is_none(),
            "isPhone/checkPhone live in one file"
        );
        let (same_file_out, _) = scan(&CloneOptions {
            same_file: true,
            ..Default::default()
        });
        let cluster = cluster_with(&same_file_out, "isPhone").expect("same-file cluster");
        assert!(cluster.members.iter().any(|m| m.name == "checkPhone"));
    }

    #[test]
    fn symbol_filter_keeps_only_clusters_containing_that_name() {
        let (out, _) = scan(&CloneOptions {
            symbol: Some("validateEmail".to_string()),
            ..Default::default()
        });
        assert_eq!(out.clusters.len(), 1);
        assert!(cluster_with(&out, "isEmail").is_some());
    }

    #[test]
    fn kind_filter_restricts_candidates_before_clustering() {
        let (out, _) = scan(&CloneOptions {
            kind: KindFilter::Fn,
            ..Default::default()
        });
        assert!(
            cluster_with(&out, "isEmail").is_none(),
            "isEmail is a const arrow; alone, validateEmail cannot cluster"
        );
        assert!(
            cluster_with(&out, "collapseWhitespace").is_some(),
            "both collapseWhitespace copies are function declarations"
        );
    }

    #[test]
    fn npm_only_common_packages_are_listed_in_the_footer() {
        let (out, _) = scan(&CloneOptions::default());
        assert_eq!(
            out.npm_only_packages,
            vec![
                "lxp-common-components-js",
                "lxp-common-constants-js",
                "lxp-common-functions-js"
            ]
        );
    }

    #[test]
    fn cluster_note_reports_existing_export_in_npm_common_package() {
        let (out, _) = scan(&CloneOptions::default());
        let cluster = cluster_with(&out, "isValidPhoneNumber").expect("phone-number pair");
        assert!(
            cluster.members.iter().any(|m| m.name == "checkPhoneNumber"),
            "the differently-named copy clusters with it"
        );
        assert!(
            cluster
                .notes
                .iter()
                .any(|n| n.contains("isValidPhoneNumber")
                    && n.contains("already exists in lxp-common-functions-js")),
            "notes: {:?}",
            cluster.notes
        );
    }

    #[test]
    fn member_in_a_cloned_common_repo_beats_the_shared_home_hint() {
        let (out, _) = scan(&CloneOptions::default());
        let cluster = cluster_with(&out, "numberWithCommas")
            .expect("app copy clusters with the cloned common repo implementation");
        assert!(
            cluster
                .members
                .iter()
                .any(|m| m.repo == "lxp-common-widgets-js" && m.name == "formatAmountWithCommas"),
            "members: {:?}",
            cluster.members
        );
        assert!(
            cluster
                .notes
                .iter()
                .any(|n| n.contains("already implemented in lxp-common-widgets-js")),
            "notes: {:?}",
            cluster.notes
        );
        assert!(
            !cluster.notes.iter().any(|n| n.contains("candidate shared home")),
            "the shared home already exists — hint would be misleading: {:?}",
            cluster.notes
        );
    }

    #[test]
    fn cluster_without_npm_hit_gets_a_shared_home_hint() {
        let (out, _) = scan(&CloneOptions::default());
        let cluster = cluster_with(&out, "isEmail").unwrap();
        assert!(
            cluster.notes.iter().any(|n| n.contains("candidate")),
            "notes: {:?}",
            cluster.notes
        );
    }

    #[test]
    fn broken_file_warns_and_never_aborts_the_scan() {
        let (_, warnings) = scan(&CloneOptions::default());
        assert!(
            warnings.iter().any(|w| w.contains("broken.ts")),
            "warnings: {warnings:?}"
        );
    }

    #[test]
    fn output_is_deterministic_across_runs() {
        let (a, _) = scan(&CloneOptions::default());
        let (b, _) = scan(&CloneOptions::default());
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
    }
}
