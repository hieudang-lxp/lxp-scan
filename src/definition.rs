use rayon::prelude::*;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::{analyzer, discover::Repo, impact::ImpactHit, walker};

/// (file, (decl_start_line, decl_end_line), file text)
type DeclCandidate = (PathBuf, (usize, usize), String);

/// Cap per section (props interface / declaration) so one giant component
/// body cannot blow up the pack.
const MAX_SECTION_LINES: usize = 30;

#[derive(Serialize, Debug)]
pub struct Definition {
    pub repo: String,
    /// path relative to the defining repo's root
    pub file: String,
    /// 1-based line of the symbol's declaration
    pub line: usize,
    /// `<symbol>Props` declaration (when in the same file) + the symbol's
    /// declaration, each capped at MAX_SECTION_LINES
    pub excerpt: String,
}

/// Locates the real declaration of `symbol` behind the hits' imports.
///
/// The defining repo is inferred per hit: a source whose first path segment
/// names a workspace repo (package specifiers, and file paths displayed with
/// a repo prefix) points at that repo; a repo-relative file path means the
/// import stayed inside the hit's own repo. The most-imported-from repo wins.
/// Barrel files never match: `analyzer::find_declaration` ignores re-exports,
/// so the scan lands on the actual `const/function/interface` declaration.
pub fn find_definition(
    symbol: &str,
    repos: &[Repo],
    hits: &[ImpactHit],
    warnings: &mut Vec<String>,
) -> Option<Definition> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for hit in hits {
        *counts.entry(defining_repo_name(hit, repos)).or_default() += 1;
    }
    let mut ranked: Vec<(&str, usize)> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let (def_repo_name, _) = *ranked.first()?;
    let repo = repos.iter().find(|r| r.name == def_repo_name)?;

    let files = walker::source_files(&repo.root);
    let scanned: Vec<Result<DeclCandidate, String>> = files
        .par_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(path).ok()?;
            if !text.contains(symbol) {
                return None;
            }
            match analyzer::find_declaration(path, &text, symbol) {
                Ok(Some(lines)) => Some(Ok((path.clone(), lines, text))),
                Ok(None) => None,
                Err(err) => Some(Err(format!("{err:#}, skipped"))),
            }
        })
        .collect();
    let mut candidates = Vec::new();
    for item in scanned {
        match item {
            Ok(candidate) => candidates.push(candidate),
            Err(warning) => warnings.push(warning),
        }
    }
    // Same-name declarations in several files are rare but real (test files,
    // storybook copies); prefer the shallowest path, then lexicographic.
    candidates.sort_by(|a, b| {
        (a.0.components().count(), &a.0).cmp(&(b.0.components().count(), &b.0))
    });
    if candidates.len() > 1 {
        warnings.push(format!(
            "{} files in {} declare {symbol}; picked {}",
            candidates.len(),
            repo.name,
            candidates[0].0.display()
        ));
    }
    let (path, (start, end), text) = candidates.into_iter().next()?;

    let mut excerpt = String::new();
    // Both props-naming conventions exist in the target repos: `AvatarProps`
    // (lxp-common) and `IAvatarProps` (lxp-web).
    for props_name in [format!("{symbol}Props"), format!("I{symbol}Props")] {
        if let Ok(Some((props_start, props_end))) =
            analyzer::find_declaration(&path, &text, &props_name)
        {
            excerpt.push_str(&slice_lines(&text, props_start, props_end));
            excerpt.push('\n');
            break;
        }
    }
    excerpt.push_str(&slice_lines(&text, start, end));

    Some(Definition {
        repo: repo.name.clone(),
        file: path
            .strip_prefix(&repo.root)
            .unwrap_or(&path)
            .to_string_lossy()
            .into_owned(),
        line: start,
        excerpt,
    })
}

/// Repo defining the component behind one hit's import: a source whose first
/// path segment names a workspace repo (package specifiers, and file paths
/// displayed with a repo prefix) points at that repo; a repo-relative file
/// path means the import stayed inside the hit's own repo.
pub fn defining_repo_name<'a>(hit: &'a ImpactHit, repos: &[Repo]) -> &'a str {
    let first = hit.source.split('/').next().unwrap_or("");
    if repos.iter().any(|r| r.name == first) {
        first
    } else {
        hit.repo.as_str()
    }
}

/// 1-based inclusive line slice, capped at MAX_SECTION_LINES with a marker.
fn slice_lines(text: &str, start: usize, end: usize) -> String {
    let capped_end = end.min(start + MAX_SECTION_LINES - 1);
    let mut out: Vec<&str> = text
        .lines()
        .skip(start - 1)
        .take(capped_end - start + 1)
        .collect();
    if capped_end < end {
        out.push("// … truncated");
    }
    let mut joined = out.join("\n");
    joined.push('\n');
    joined
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{discover, impact};
    use std::path::PathBuf;

    fn workspace() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/workspace")
    }

    #[test]
    fn finds_definition_behind_barrel_reexport() {
        let mut warnings = Vec::new();
        let repos = discover::discover_repos(&workspace(), &mut warnings).unwrap();
        let hits = impact::run_impact(&workspace(), "Button", None, &mut warnings).unwrap();
        let def = find_definition("Button", &repos, &hits, &mut warnings).unwrap();
        assert_eq!(def.repo, "fake-lib");
        assert_eq!(def.file, "components/Button/Button.tsx");
        assert_eq!(def.line, 8);
        assert!(def.excerpt.contains("export interface ButtonProps"));
        assert!(def.excerpt.contains("export const Button"));
        assert!(!def.excerpt.contains("export * from"));
    }

    #[test]
    fn intra_repo_source_resolves_to_the_importing_repo() {
        let mut warnings = Vec::new();
        let repos = discover::discover_repos(&workspace(), &mut warnings).unwrap();
        let hits = impact::run_impact(&workspace(), "formatThing", None, &mut warnings).unwrap();
        let def = find_definition("formatThing", &repos, &hits, &mut warnings).unwrap();
        assert_eq!(def.repo, "app-one");
        assert_eq!(def.file, "src/utils/helpers.ts");
        assert!(def.excerpt.contains("export const formatThing"));
    }

    #[test]
    fn i_prefixed_props_interface_is_included_in_the_excerpt() {
        let mut warnings = Vec::new();
        let repos = discover::discover_repos(&workspace(), &mut warnings).unwrap();
        let hits = vec![ImpactHit {
            repo: "app-one".to_string(),
            file: "src/anywhere.tsx".to_string(),
            line: 1,
            source: "fake-lib/components/Card".to_string(),
            refs: 0,
            jsx_uses: 1,
            jsx_props: Default::default(),
            jsx_lines: vec![1],
        }];
        let def = find_definition("Card", &repos, &hits, &mut warnings).unwrap();
        assert_eq!(def.repo, "fake-lib");
        assert!(def.excerpt.contains("export interface ICardProps"));
        assert!(def.excerpt.contains("export const Card"));
    }

    #[test]
    fn no_declaration_anywhere_returns_none() {
        let mut warnings = Vec::new();
        let repos = discover::discover_repos(&workspace(), &mut warnings).unwrap();
        let hits = impact::run_impact(&workspace(), "Button", None, &mut warnings).unwrap();
        assert!(find_definition("NoSuchSymbol", &repos, &hits, &mut warnings).is_none());
    }

    #[test]
    fn long_declarations_are_truncated() {
        let text = (1..=50)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let sliced = slice_lines(&text, 1, 50);
        assert_eq!(sliced.lines().count(), MAX_SECTION_LINES + 1);
        assert!(sliced.ends_with("// … truncated\n"));
    }
}
