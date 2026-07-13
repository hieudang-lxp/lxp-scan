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
fn from_filter_narrows_by_source() {
    let mut warnings = Vec::new();
    let all = lxp_scan::impact::run_impact(&workspace(), "Button", None, &mut warnings).unwrap();
    let filtered =
        lxp_scan::impact::run_impact(&workspace(), "Button", Some("fake-lib"), &mut warnings)
            .unwrap();
    assert_eq!(all.len(), filtered.len()); // all Button imports come from fake-lib
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
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].repo, "app-one");
}
