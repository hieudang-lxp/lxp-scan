use crate::scan::discover::Repo;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize, PartialEq, Eq, Debug, Clone, Copy)]
pub enum DriftLevel {
    Same,
    Minor,
    Major,
}

#[derive(Serialize, Debug)]
pub struct DriftRow {
    pub package: String,
    /// repo name -> version string
    pub versions: BTreeMap<String, String>,
    pub level: DriftLevel,
}

pub const TRACKED_PREFIXES: [&str; 2] = ["lxp-common-", "lxp-design-system"];

fn major_minor(version: &str) -> Option<(u64, u64)> {
    let v = version.trim_start_matches(['^', '~', '=', 'v']);
    let mut parts = v.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().unwrap_or(0);
    Some((major, minor))
}

pub fn compute_drift(repos: &[Repo]) -> Vec<DriftRow> {
    let mut by_package: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for repo in repos {
        for (pkg, ver) in &repo.deps {
            if TRACKED_PREFIXES.iter().any(|p| pkg.starts_with(p)) {
                by_package
                    .entry(pkg.clone())
                    .or_default()
                    .insert(repo.name.clone(), ver.clone());
            }
        }
    }
    by_package
        .into_iter()
        .map(|(package, versions)| {
            let parsed: Vec<_> = versions.values().filter_map(|v| major_minor(v)).collect();
            let level = if parsed.windows(2).any(|w| w[0].0 != w[1].0) {
                DriftLevel::Major
            } else if parsed.windows(2).any(|w| w[0].1 != w[1].1) {
                DriftLevel::Minor
            } else {
                DriftLevel::Same
            };
            DriftRow {
                package,
                versions,
                level,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn repo(name: &str, deps: &[(&str, &str)]) -> Repo {
        Repo {
            name: name.into(),
            root: PathBuf::from("/tmp").join(name),
            deps: deps
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    #[test]
    fn flags_major_drift() {
        let repos = [
            repo("a", &[("lxp-common-components-js", "^3.1.32")]),
            repo("b", &[("lxp-common-components-js", "^2.1.56")]),
        ];
        let rows = compute_drift(&repos);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].level, DriftLevel::Major);
    }

    #[test]
    fn flags_minor_drift() {
        let repos = [
            repo("a", &[("lxp-common-permissions-js", "^1.1.59")]),
            repo("b", &[("lxp-common-permissions-js", "^1.2.10")]),
        ];
        let rows = compute_drift(&repos);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].level, DriftLevel::Minor);
    }

    #[test]
    fn same_version_is_same_and_untracked_packages_are_ignored() {
        let repos = [
            repo(
                "a",
                &[("lxp-common-constants-js", "^1.0.24"), ("react", "^18.2.0")],
            ),
            repo(
                "b",
                &[("lxp-common-constants-js", "^1.0.24"), ("react", "^17.0.2")],
            ),
        ];
        let rows = compute_drift(&repos);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].package, "lxp-common-constants-js");
        assert_eq!(rows[0].level, DriftLevel::Same);
    }

    #[test]
    fn package_present_in_only_one_repo_is_listed() {
        let repos = [
            repo("a", &[("lxp-common-hooks-js", "^0.0.8")]),
            repo("b", &[]),
        ];
        let rows = compute_drift(&repos);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].versions.len(), 1);
        assert_eq!(rows[0].level, DriftLevel::Same);
    }
}
