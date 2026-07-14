use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub enum ResolvedImport {
    /// npm package specifier, kept verbatim (e.g. "fake-lib/components/Button")
    Package(String),
    /// canonical absolute path of an intra-repo module (extension resolved when the file exists)
    File(PathBuf),
}

/// One tsconfig `paths` entry, star-trimmed. `wildcard` records whether the
/// original pattern ended with `*`: wildcard patterns prefix-match, exact
/// patterns match only on full string equality.
struct PathAlias {
    prefix: String,
    wildcard: bool,
    targets: Vec<PathBuf>,
}

pub struct RepoResolver {
    repo_root: PathBuf,
    base_url: Option<PathBuf>,
    /// Sorted by prefix length descending so the longest prefix wins,
    /// matching TypeScript's paths-resolution semantics.
    paths: Vec<PathAlias>,
}

impl RepoResolver {
    /// Reads <repo_root>/tsconfig.json (JSONC-tolerant). Missing/broken tsconfig
    /// pushes a warning and yields a resolver with no aliases.
    pub fn from_repo(repo_root: &Path, warnings: &mut Vec<String>) -> Self {
        let empty = Self {
            repo_root: repo_root.to_path_buf(),
            base_url: None,
            paths: Vec::new(),
        };
        let tsconfig_path = repo_root.join("tsconfig.json");
        let text = match std::fs::read_to_string(&tsconfig_path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                warnings.push(format!(
                    "{}: no tsconfig.json, alias resolution disabled",
                    repo_root.display()
                ));
                return empty;
            }
            Err(err) => {
                warnings.push(format!(
                    "{}: unreadable tsconfig.json ({err}), alias resolution disabled",
                    tsconfig_path.display()
                ));
                return empty;
            }
        };
        let parsed: Result<serde_json::Value, _> =
            jsonc_parser::parse_to_serde_value(&text, &Default::default());
        let value = match parsed {
            Ok(value) => value,
            Err(err) => {
                warnings.push(format!(
                    "{}: unreadable tsconfig.json ({err}), alias resolution disabled",
                    tsconfig_path.display()
                ));
                return empty;
            }
        };
        let compiler_options = value.get("compilerOptions");
        let base_url = compiler_options
            .and_then(|opts| opts.get("baseUrl"))
            .and_then(|v| v.as_str())
            .map(|base| normalize(&repo_root.join(base)));
        let alias_root = base_url.clone().unwrap_or_else(|| repo_root.to_path_buf());
        let mut paths = Vec::new();
        if let Some(map) = compiler_options
            .and_then(|opts| opts.get("paths"))
            .and_then(|v| v.as_object())
        {
            for (pattern, targets) in map {
                let Some(targets) = targets.as_array() else {
                    continue;
                };
                let wildcard = pattern.ends_with('*');
                let prefix = pattern.trim_end_matches('*').to_string();
                let targets = targets
                    .iter()
                    .filter_map(|t| t.as_str())
                    .map(|t| normalize(&alias_root.join(t.trim_end_matches('*'))))
                    .collect();
                paths.push(PathAlias {
                    prefix,
                    wildcard,
                    targets,
                });
            }
        }
        paths.sort_by_key(|alias| std::cmp::Reverse(alias.prefix.len()));
        Self {
            repo_root: repo_root.to_path_buf(),
            base_url,
            paths,
        }
    }

    /// importer: absolute path of the importing file.
    pub fn resolve(&self, importer: &Path, specifier: &str) -> ResolvedImport {
        if specifier.starts_with('.') {
            let base = importer.parent().unwrap_or(importer).join(specifier);
            let normalized = normalize(&base);
            let resolved = resolve_extension(&normalized).unwrap_or(normalized);
            return ResolvedImport::File(resolved);
        }
        for alias in &self.paths {
            let Some(rest) = specifier.strip_prefix(alias.prefix.as_str()) else {
                continue;
            };
            if !alias.wildcard && !rest.is_empty() {
                continue;
            }
            for target in &alias.targets {
                let candidate = if rest.is_empty() {
                    target.clone()
                } else {
                    normalize(&target.join(rest))
                };
                if let Some(resolved) = resolve_extension(&candidate) {
                    return ResolvedImport::File(resolved);
                }
            }
        }
        if let Some(base_url) = &self.base_url {
            let candidate = normalize(&base_url.join(specifier));
            if let Some(resolved) = resolve_extension(&candidate) {
                return ResolvedImport::File(resolved);
            }
        }
        ResolvedImport::Package(specifier.to_string())
    }

    /// Display: packages verbatim; files as repo-name-prefixed repo-relative
    /// paths (`lxp-web/src/components/X/index.tsx`). The prefix keeps sources
    /// unambiguous across repos — two repos routinely contain identically
    /// shaped local paths — and makes the display independent of whether
    /// `--root` was given relative or absolute (a bare relative root used to
    /// fail the strip and leak a prefixed path by accident).
    pub fn display(&self, resolved: &ResolvedImport) -> String {
        match resolved {
            ResolvedImport::Package(name) => name.clone(),
            ResolvedImport::File(path) => {
                let stripped = path
                    .strip_prefix(&self.repo_root)
                    .or_else(|_| path.strip_prefix(normalize(&self.repo_root)))
                    .ok();
                match (stripped, self.repo_root.file_name()) {
                    (Some(rel), Some(repo)) => {
                        Path::new(repo).join(rel).to_string_lossy().into_owned()
                    }
                    _ => path.to_string_lossy().into_owned(),
                }
            }
        }
    }
}

/// Pure lexical normalization: `..` pops, `.` is dropped, no filesystem access.
fn normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Probes `<base>`, then `<base>.{ts,tsx,js,jsx}`, then `<base>/index.{ts,tsx,js,jsx}`.
/// Extensions are appended (not `with_extension`) so specifiers containing dots
/// (e.g. "foo.util") are not clobbered.
fn resolve_extension(base: &Path) -> Option<PathBuf> {
    const EXTENSIONS: [&str; 4] = ["ts", "tsx", "js", "jsx"];
    if base.is_file() {
        return Some(base.to_path_buf());
    }
    for ext in EXTENSIONS {
        let candidate = PathBuf::from(format!("{}.{ext}", base.display()));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    for ext in EXTENSIONS {
        let candidate = base.join(format!("index.{ext}"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(repo: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/workspace")
            .join(repo)
    }

    /// Builds the app-one resolver, asserting the JSONC tsconfig parses cleanly.
    fn resolver() -> RepoResolver {
        let mut warnings = Vec::new();
        let resolver = RepoResolver::from_repo(&fixture("app-one"), &mut warnings);
        assert_eq!(
            warnings,
            Vec::<String>::new(),
            "app-one tsconfig should parse without warnings"
        );
        resolver
    }

    #[test]
    fn package_specifier_stays_package() {
        let resolver = resolver();
        let importer = fixture("app-one").join("src/page.tsx");
        assert_eq!(
            resolver.resolve(&importer, "fake-lib/components/Button"),
            ResolvedImport::Package("fake-lib/components/Button".to_string())
        );
    }

    #[test]
    fn paths_alias_resolves_to_existing_file() {
        let resolver = resolver();
        let importer = fixture("app-one").join("src/page.tsx");
        assert_eq!(
            resolver.resolve(&importer, "utils/helpers"),
            ResolvedImport::File(fixture("app-one").join("src/utils/helpers.ts"))
        );
    }

    #[test]
    fn relative_import_canonicalizes_same_as_alias() {
        let resolver = resolver();
        let importer = fixture("app-one").join("src/page.tsx");
        let via_relative = resolver.resolve(&importer, "./utils/helpers");
        let via_alias = resolver.resolve(&importer, "utils/helpers");
        assert_eq!(via_relative, via_alias);
        assert!(matches!(via_relative, ResolvedImport::File(_)));
    }

    #[test]
    fn display_is_repo_prefixed_for_files_and_verbatim_for_packages() {
        let resolver = resolver();
        let importer = fixture("app-one").join("src/page.tsx");
        let file = resolver.resolve(&importer, "utils/helpers");
        // repo prefix keeps same-shaped paths in different repos distinguishable
        // (two repos both having src/components/X/index.tsx is common), so a
        // --from hint can never match the wrong repo
        assert_eq!(resolver.display(&file), "app-one/src/utils/helpers.ts");
        let package = ResolvedImport::Package("fake-lib/components/Button".to_string());
        assert_eq!(resolver.display(&package), "fake-lib/components/Button");
    }

    #[test]
    fn exact_paths_pattern_matches_only_on_full_equality() {
        let root = fixture("app-one");
        // Mirrors real tsconfigs like {"@components": ["src/components"]}:
        // a star-less pattern must not act as a prefix.
        let resolver = RepoResolver {
            repo_root: root.clone(),
            base_url: None,
            paths: vec![PathAlias {
                prefix: "@utils".to_string(),
                wildcard: false,
                targets: vec![root.join("src/utils")],
            }],
        };
        let importer = root.join("src/page.tsx");
        // Exact match resolves (via index probing under the target directory
        // it would be a dir; here probe the file directly).
        let helpers_resolver = RepoResolver {
            repo_root: root.clone(),
            base_url: None,
            paths: vec![PathAlias {
                prefix: "@helpers".to_string(),
                wildcard: false,
                targets: vec![root.join("src/utils/helpers")],
            }],
        };
        assert_eq!(
            helpers_resolver.resolve(&importer, "@helpers"),
            ResolvedImport::File(root.join("src/utils/helpers.ts"))
        );
        // A longer specifier that merely starts with the exact pattern must
        // NOT match it (buggy prefix-matching would land on src/utils/helpers).
        assert_eq!(
            resolver.resolve(&importer, "@utilshelpers"),
            ResolvedImport::Package("@utilshelpers".to_string())
        );
    }

    #[test]
    fn base_url_resolves_bare_specifier_not_covered_by_paths() {
        let resolver = resolver();
        let importer = fixture("app-one").join("src/utils/helpers.ts");
        assert_eq!(
            resolver.resolve(&importer, "page"),
            ResolvedImport::File(fixture("app-one").join("src/page.tsx"))
        );
    }

    #[test]
    fn missing_tsconfig_only_warns_and_still_resolves_relative() {
        let mut warnings = Vec::new();
        let resolver = RepoResolver::from_repo(&fixture("app-two"), &mut warnings);
        assert_eq!(
            warnings.len(),
            1,
            "expected exactly one warning, got: {warnings:?}"
        );
        let importer = fixture("app-two").join("src/other.tsx");
        assert_eq!(
            resolver.resolve(&importer, "../src/whole"),
            ResolvedImport::File(fixture("app-two").join("src/whole.ts"))
        );
    }
}
