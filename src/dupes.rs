use rayon::prelude::*;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

use crate::{analyzer, discover, walker};

/// exported values of one file: (name, decl line, repo-relative file)
type FileExports = Vec<(String, usize, String)>;

#[derive(Serialize, Debug)]
pub struct DeclSite {
    pub repo: String,
    /// path relative to the repo root
    pub file: String,
    pub line: usize,
}

#[derive(Serialize, Debug)]
pub struct DupeGroup {
    pub name: String,
    pub sites: Vec<DeclSite>,
    pub repo_count: usize,
}

/// Same-name exported values declared in more than one repo — candidates for
/// consolidation into lxp-common. Only component-shaped names (leading
/// uppercase) are reported: lowercase utils dupe too, but the uppercase set
/// is where two teams ship parallel implementations of the same UI.
/// Names ending in `Props` are excluded (conventional per-component types).
pub fn find_dupes(root: &Path, warnings: &mut Vec<String>) -> anyhow::Result<Vec<DupeGroup>> {
    let repos = discover::discover_repos(root, warnings)?;
    let mut by_name: BTreeMap<String, Vec<DeclSite>> = BTreeMap::new();
    for repo in &repos {
        let files = walker::source_files(&repo.root);
        let scanned: Vec<Result<FileExports, String>> = files
            .par_iter()
            .filter_map(|path| {
                // stories and tests export copies of the names they exercise —
                // they are consumers, not parallel implementations
                let name = path.to_string_lossy();
                if name.contains(".stories.")
                    || name.contains(".test.")
                    || name.contains(".spec.")
                    || name.contains("__test__")
                    || name.contains("__tests__")
                {
                    return None;
                }
                let text = std::fs::read_to_string(path).ok()?;
                if !text.contains("export") {
                    return None;
                }
                let file = path
                    .strip_prefix(&repo.root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .into_owned();
                match analyzer::exported_values(path, &text) {
                    Ok(exports) => Some(Ok(exports
                        .into_iter()
                        .filter(|(name, _)| is_component_shaped(name))
                        .map(|(name, line)| (name, line, file.clone()))
                        .collect())),
                    Err(err) => Some(Err(format!("{err:#}, skipped"))),
                }
            })
            .collect();
        for item in scanned {
            match item {
                Ok(exports) => {
                    for (name, line, file) in exports {
                        by_name.entry(name).or_default().push(DeclSite {
                            repo: repo.name.clone(),
                            file,
                            line,
                        });
                    }
                }
                Err(warning) => warnings.push(warning),
            }
        }
    }

    let mut groups: Vec<DupeGroup> = by_name
        .into_iter()
        .filter_map(|(name, mut sites)| {
            sites.sort_by(|a, b| (&a.repo, &a.file, a.line).cmp(&(&b.repo, &b.file, b.line)));
            let repo_count = sites
                .iter()
                .map(|s| s.repo.as_str())
                .collect::<std::collections::BTreeSet<_>>()
                .len();
            (repo_count >= 2).then_some(DupeGroup {
                name,
                sites,
                repo_count,
            })
        })
        .collect();
    groups.sort_by(|a, b| b.repo_count.cmp(&a.repo_count).then(a.name.cmp(&b.name)));
    Ok(groups)
}

fn is_component_shaped(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && !name.ends_with("Props")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn workspace() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/workspace")
    }

    #[test]
    fn reports_names_declared_in_multiple_repos_only() {
        let mut warnings = Vec::new();
        let groups = find_dupes(&workspace(), &mut warnings).unwrap();
        let button = groups
            .iter()
            .find(|g| g.name == "Button")
            .expect("Button is declared in app-one, app-two and fake-lib");
        assert_eq!(button.repo_count, 3);
        assert!(button.sites.iter().any(|s| s.repo == "app-one" && s.file.ends_with("helpers.ts")));
        assert!(button.sites.iter().any(|s| s.repo == "app-two" && s.file.ends_with("local-button.tsx")));
        assert!(button.sites.iter().any(|s| s.repo == "fake-lib" && s.file.ends_with("Button.tsx")));
        // a story exporting Button is not a parallel implementation
        assert!(
            !button.sites.iter().any(|s| s.file.contains(".stories.")),
            "stories must not count as declaration sites: {:?}",
            button.sites
        );
        // Card exists only in fake-lib — not a dupe
        assert!(!groups.iter().any(|g| g.name == "Card"));
        // lowercase utils and *Props types are filtered out
        assert!(!groups.iter().any(|g| g.name == "formatThing"));
        assert!(!groups.iter().any(|g| g.name.ends_with("Props")));
    }

    #[test]
    fn groups_sort_by_repo_count_then_name() {
        let mut warnings = Vec::new();
        let groups = find_dupes(&workspace(), &mut warnings).unwrap();
        let counts: Vec<usize> = groups.iter().map(|g| g.repo_count).collect();
        let mut sorted = counts.clone();
        sorted.sort_by(|a, b| b.cmp(a));
        assert_eq!(counts, sorted);
    }
}
