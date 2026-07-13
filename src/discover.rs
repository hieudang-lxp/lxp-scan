use anyhow::Result;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct Repo {
    pub name: String,
    pub root: PathBuf,
    /// dependencies + devDependencies merged
    pub deps: BTreeMap<String, String>,
}

pub fn discover_repos(root: &Path, warnings: &mut Vec<String>) -> Result<Vec<Repo>> {
    let entries = std::fs::read_dir(root)
        .map_err(|e| anyhow::anyhow!("cannot read root {}: {e}", root.display()))?;
    let mut repos = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let pkg = dir.join("package.json");
        if !pkg.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let text = match std::fs::read_to_string(&pkg) {
            Ok(t) => t,
            Err(e) => {
                warnings.push(format!("{name}: cannot read package.json: {e}"));
                continue;
            }
        };
        let value: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                warnings.push(format!("{name}: malformed package.json: {e}"));
                continue;
            }
        };
        let mut deps = BTreeMap::new();
        for key in ["dependencies", "devDependencies"] {
            if let Some(map) = value.get(key).and_then(|v| v.as_object()) {
                for (k, v) in map {
                    if let Some(ver) = v.as_str() {
                        deps.insert(k.clone(), ver.to_string());
                    }
                }
            }
        }
        repos.push(Repo {
            name,
            root: dir,
            deps,
        });
    }
    repos.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(repos)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/workspace")
    }

    #[test]
    fn finds_only_dirs_with_package_json() {
        let repos = discover_repos(&fixture_root(), &mut vec![]).unwrap();
        let names: Vec<_> = repos.iter().map(|r| r.name.as_str()).collect();
        // broken-repo is excluded because its package.json is malformed
        assert_eq!(names, ["app-one", "app-two"]);
    }

    #[test]
    fn merges_dependencies_and_dev_dependencies() {
        let repos = discover_repos(&fixture_root(), &mut vec![]).unwrap();
        let one = repos.iter().find(|r| r.name == "app-one").unwrap();
        assert_eq!(one.deps["lxp-common-components-js"], "^3.1.32");
        assert_eq!(one.deps["lxp-common-constants-js"], "^1.0.24");
    }

    #[test]
    fn nonexistent_root_is_an_error() {
        assert!(discover_repos(Path::new("/definitely/not/here"), &mut vec![]).is_err());
    }

    #[test]
    fn malformed_package_json_warns_and_skips() {
        let mut warnings = vec![];
        let repos = discover_repos(&fixture_root(), &mut warnings).unwrap();
        assert!(!repos.iter().any(|r| r.name == "broken-repo"));
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("broken-repo") && w.contains("malformed"))
        );
    }
}
