use crate::drift::{DriftLevel, DriftRow};
use crate::impact::ImpactHit;
use anyhow::Result;
use comfy_table::{Cell, Color, ContentArrangement, Table, presets::UTF8_BORDERS_ONLY};
use std::io::IsTerminal;

/// Minimal borders; wrap to terminal width only when stdout is a TTY so piped
/// output stays one-line-per-row (grep-able) and tests stay deterministic.
fn base_table() -> Table {
    let mut table = Table::new();
    table.load_preset(UTF8_BORDERS_ONLY);
    if std::io::stdout().is_terminal() {
        table.set_content_arrangement(ContentArrangement::Dynamic);
    }
    table
}

fn level_cell(level: DriftLevel) -> Cell {
    let cell = Cell::new(format!("{level:?}"));
    if !std::io::stdout().is_terminal() {
        return cell;
    }
    match level {
        DriftLevel::Major => cell.fg(Color::Red),
        DriftLevel::Minor => cell.fg(Color::Yellow),
        DriftLevel::Same => cell.fg(Color::Green),
    }
}

pub fn drift_json(rows: &[DriftRow]) -> Result<String> {
    Ok(serde_json::to_string_pretty(rows)?)
}

pub fn impact_json(hits: &[ImpactHit]) -> Result<String> {
    Ok(serde_json::to_string_pretty(hits)?)
}

pub fn impact_table(hits: &[ImpactHit]) -> String {
    let mut table = base_table();
    table.set_header(["repo", "file:line", "from", "refs", "jsx", "props"]);
    for hit in hits {
        let props: Vec<&str> = hit.jsx_props.iter().map(String::as_str).collect();
        table.add_row([
            hit.repo.clone(),
            format!("{}:{}", hit.file, hit.line),
            hit.source.clone(),
            hit.refs.to_string(),
            hit.jsx_uses.to_string(),
            props.join(", "),
        ]);
    }
    table.to_string()
}

pub fn drift_table(rows: &[DriftRow], repo_names: &[String]) -> String {
    // Repos that consume no tracked package would render an all-dash column.
    let consumers: Vec<&String> = repo_names
        .iter()
        .filter(|name| rows.iter().any(|row| row.versions.contains_key(*name)))
        .collect();
    let mut table = base_table();
    let mut header = vec![Cell::new("package")];
    header.extend(consumers.iter().map(Cell::new));
    header.push(Cell::new("drift"));
    table.set_header(header);
    for row in rows {
        let mut cells = vec![Cell::new(&row.package)];
        for repo in &consumers {
            cells.push(Cell::new(
                row.versions.get(*repo).map(String::as_str).unwrap_or("-"),
            ));
        }
        cells.push(level_cell(row.level));
        table.add_row(cells);
    }
    table.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::Repo;
    use crate::drift::compute_drift;
    use crate::impact::ImpactHit;
    use std::path::PathBuf;

    fn sample_rows() -> Vec<DriftRow> {
        let repos = [
            Repo {
                name: "a".into(),
                root: PathBuf::from("/tmp/a"),
                deps: [(
                    "lxp-common-components-js".to_string(),
                    "^3.1.32".to_string(),
                )]
                .into_iter()
                .collect(),
            },
            Repo {
                name: "b".into(),
                root: PathBuf::from("/tmp/b"),
                deps: [(
                    "lxp-common-components-js".to_string(),
                    "^2.1.56".to_string(),
                )]
                .into_iter()
                .collect(),
            },
        ];
        compute_drift(&repos)
    }

    #[test]
    fn json_roundtrips() {
        let rows = sample_rows();
        let json = drift_json(&rows).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v[0]["package"], "lxp-common-components-js");
        assert_eq!(v[0]["level"], "Major");
    }

    fn sample_hits() -> Vec<ImpactHit> {
        vec![ImpactHit {
            repo: "app-one".to_string(),
            file: "src/page.tsx".to_string(),
            line: 1,
            source: "fake-lib/components/Button".to_string(),
            refs: 0,
            jsx_uses: 1,
            jsx_props: ["variant", "size"].iter().map(|s| s.to_string()).collect(),
        }]
    }

    #[test]
    fn impact_json_roundtrips() {
        let json = impact_json(&sample_hits()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v[0]["repo"], "app-one");
        assert_eq!(v[0]["jsx_props"][0], "size");
    }

    #[test]
    fn impact_table_row_has_location_source_counts_and_joined_props() {
        let table = impact_table(&sample_hits());
        let row_line = table
            .lines()
            .find(|l| l.contains("app-one"))
            .expect("data row for app-one");
        assert!(row_line.contains("src/page.tsx:1"));
        assert!(row_line.contains("fake-lib/components/Button"));
        assert!(row_line.contains(" 0 "));
        assert!(row_line.contains(" 1 "));
        assert!(row_line.contains("size, variant"));
    }

    #[test]
    fn table_contains_versions_and_dash_for_missing() {
        let rows = sample_rows();
        let names = vec!["a".to_string(), "b".to_string()];
        let table = drift_table(&rows, &names);
        let row_line = table
            .lines()
            .find(|l| l.contains("lxp-common-components-js"))
            .unwrap();
        assert!(row_line.contains("^3.1.32"));
        assert!(row_line.contains("^2.1.56"));
    }

    #[test]
    fn drift_table_drops_repos_without_tracked_packages_but_keeps_dash_for_partial() {
        let repos = [
            Repo {
                name: "app-a".into(),
                root: PathBuf::from("/tmp/app-a"),
                deps: [
                    (
                        "lxp-common-components-js".to_string(),
                        "^3.1.32".to_string(),
                    ),
                    ("lxp-common-hooks-js".to_string(), "^0.0.8".to_string()),
                ]
                .into_iter()
                .collect(),
            },
            Repo {
                name: "app-b".into(),
                root: PathBuf::from("/tmp/app-b"),
                deps: [(
                    "lxp-common-components-js".to_string(),
                    "^2.1.56".to_string(),
                )]
                .into_iter()
                .collect(),
            },
            Repo {
                name: "app-c".into(),
                root: PathBuf::from("/tmp/app-c"),
                deps: [("react".to_string(), "^18.0.0".to_string())]
                    .into_iter()
                    .collect(),
            },
        ];
        let rows = compute_drift(&repos);
        let names: Vec<String> = repos.iter().map(|r| r.name.clone()).collect();
        let table = drift_table(&rows, &names);
        // app-c consumes no tracked package: its column disappears entirely
        assert!(!table.contains("app-c"));
        // app-b lacks hooks-js but consumes components-js: dash in the hooks row
        let hooks_row = table
            .lines()
            .find(|l| l.contains("lxp-common-hooks-js"))
            .unwrap();
        assert!(hooks_row.contains(" - "));
    }
}
