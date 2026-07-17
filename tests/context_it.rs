use std::path::PathBuf;

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/workspace")
}

#[test]
fn markdown_pack_has_definition_props_and_excerpts() {
    let mut warnings = Vec::new();
    let pack =
        lxp_scan::features::context::build_context(&workspace(), "Button", None, 8, &mut warnings).unwrap();
    let md = lxp_scan::output::report::context_markdown(&pack, "fixtures");

    assert!(md.starts_with("# Context: Button\n"));
    assert!(md.contains("Scanned fixtures ·"));
    // definition resolved through the barrel file to the real declaration
    assert!(md.contains("fake-lib/components/Button/Button.tsx:8"));
    assert!(md.contains("export interface ButtonProps"));
    assert!(md.contains("## Props observed across usages"));
    assert!(md.contains("variant ×"));
    assert!(md.contains("## Usage excerpts"));
    assert!(md.contains("### app-one · src/page.tsx:"));
    // excerpt code is fenced and anchored at the render site
    assert!(md.contains("```tsx\n  <Button variant=\"primary\""));
}

#[test]
fn json_pack_roundtrips() {
    let mut warnings = Vec::new();
    let pack =
        lxp_scan::features::context::build_context(&workspace(), "Button", None, 8, &mut warnings).unwrap();
    let json = lxp_scan::output::report::context_json(&pack).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["symbol"], "Button");
    assert_eq!(v["definition"]["repo"], "fake-lib");
    assert!(v["excerpts"].as_array().unwrap().len() >= 2);
}
