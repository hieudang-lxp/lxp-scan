use std::path::PathBuf;

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/workspace")
}

#[test]
fn finds_button_across_both_apps() {
    let mut warnings = Vec::new();
    let hits = lxp_scan::impact::run_impact(&workspace(), "Button", None, &mut warnings).unwrap();
    let repos: Vec<_> = hits.iter().map(|h| h.repo.as_str()).collect();
    assert!(repos.contains(&"app-one"));
    assert!(repos.contains(&"app-two"));
    let one = hits.iter().find(|h| h.repo == "app-one").unwrap();
    assert!(one.jsx_props.contains("variant"));
    // helpers.ts defines a local `Button` const — must NOT appear as a hit
    assert!(!hits.iter().any(|h| h.file.ends_with("helpers.ts")));
}

#[test]
fn warns_on_unparseable_file_but_keeps_other_hits() {
    let mut warnings = Vec::new();
    let hits = lxp_scan::impact::run_impact(&workspace(), "Button", None, &mut warnings).unwrap();
    // broken.ts is unparseable: it must warn, not kill the scan
    assert!(
        warnings.iter().any(|w| w.contains("broken.ts")),
        "expected a warning mentioning broken.ts, got: {warnings:?}"
    );
    // the healthy file in the same repo is still reported
    assert!(
        hits.iter()
            .any(|h| h.repo == "app-two" && h.file.ends_with("other.tsx")),
        "expected the app-two other.tsx hit to survive, got: {hits:?}"
    );
}

#[test]
fn from_filter_narrows_by_source() {
    let mut warnings = Vec::new();
    let all = lxp_scan::impact::run_impact(&workspace(), "Button", None, &mut warnings).unwrap();
    assert!(!all.is_empty());
    let filtered =
        lxp_scan::impact::run_impact(&workspace(), "Button", Some("fake-lib"), &mut warnings)
            .unwrap();
    // everything except app-two's local Button (src/special.tsx) is from fake-lib
    assert_eq!(all.len(), filtered.len() + 1);
    assert!(filtered.iter().all(|h| h.source.contains("fake-lib")));
    let none =
        lxp_scan::impact::run_impact(&workspace(), "Button", Some("no-such-lib"), &mut warnings)
            .unwrap();
    assert!(none.is_empty());
}

#[test]
fn alias_and_relative_hits_match_same_from_filter() {
    let mut warnings = Vec::new();
    let hits = lxp_scan::impact::run_impact(
        &workspace(),
        "formatThing",
        Some("utils/helpers"),
        &mut warnings,
    )
    .unwrap();
    // page.tsx imports via the tsconfig alias, section.tsx via a relative
    // path; both must resolve to the same display and match the same filter
    assert_eq!(hits.len(), 2, "expected alias + relative hits: {hits:?}");
    for hit in &hits {
        assert_eq!(hit.repo, "app-one");
        assert_eq!(hit.source, "app-one/src/utils/helpers.ts");
    }
    assert!(hits.iter().any(|h| h.file.ends_with("page.tsx")));
    assert!(hits.iter().any(|h| h.file.ends_with("section.tsx")));
}
