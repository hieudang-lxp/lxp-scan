use crate::drift::DriftRow;
use crate::impact::ImpactHit;
use anyhow::Result;
use comfy_table::Table;

pub fn drift_json(rows: &[DriftRow]) -> Result<String> {
    Ok(serde_json::to_string_pretty(rows)?)
}

pub fn impact_json(hits: &[ImpactHit]) -> Result<String> {
    Ok(serde_json::to_string_pretty(hits)?)
}

pub fn impact_table(hits: &[ImpactHit]) -> String {
    let mut table = Table::new();
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
    let mut table = Table::new();
    let mut header = vec!["package".to_string()];
    header.extend(repo_names.iter().cloned());
    header.push("drift".to_string());
    table.set_header(header);
    for row in rows {
        let mut cells = vec![row.package.clone()];
        for repo in repo_names {
            cells.push(
                row.versions
                    .get(repo)
                    .cloned()
                    .unwrap_or_else(|| "-".to_string()),
            );
        }
        cells.push(format!("{:?}", row.level));
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
        let names = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let table = drift_table(&rows, &names);
        let row_line = table
            .lines()
            .find(|l| l.contains("lxp-common-components-js"))
            .unwrap();
        assert!(row_line.contains("^3.1.32"));
        assert!(row_line.contains("^2.1.56"));
        assert!(row_line.contains(" - ")); // repo c has no version
    }
}
