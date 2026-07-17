use crate::features::clones::ClonesOutput;
use crate::features::context::ContextPack;
use crate::features::drift::{DriftLevel, DriftRow};
use crate::features::dupes::DupeGroup;
use crate::features::impact::ImpactHit;
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

/// Grouped, borderless report: one header per repo, two lines per hit.
/// Tables broke down here — long paths plus prop lists either wrapped into
/// multi-line cells (TTY) or produced 400-char rows (piped).
pub fn impact_report(hits: &[ImpactHit]) -> String {
    let mut out = String::new();
    let mut current_repo: Option<&str> = None;
    for hit in hits {
        if current_repo != Some(hit.repo.as_str()) {
            if current_repo.is_some() {
                out.push('\n');
            }
            let sites = hits.iter().filter(|h| h.repo == hit.repo).count();
            let plural = if sites == 1 { "" } else { "s" };
            out.push_str(&format!("{} ({sites} site{plural})\n", hit.repo));
            current_repo = Some(hit.repo.as_str());
        }
        out.push_str(&format!("  {}:{}\n", hit.file, hit.line));
        let mut parts = Vec::new();
        if hit.refs > 0 {
            parts.push(format!("ref ×{}", hit.refs));
        }
        if hit.jsx_uses > 0 {
            parts.push(format!("jsx ×{}", hit.jsx_uses));
        }
        if parts.is_empty() {
            parts.push("import only".to_string());
        }
        parts.push(format!("from {}", hit.source));
        if !hit.jsx_props.is_empty() {
            let props: Vec<&str> = hit.jsx_props.iter().map(String::as_str).collect();
            parts.push(format!("props: {}", props.join(", ")));
        }
        out.push_str(&format!("      {}\n", parts.join(" · ")));
    }
    out
}

pub fn dupes_json(groups: &[DupeGroup]) -> Result<String> {
    Ok(serde_json::to_string_pretty(groups)?)
}

/// Same grouped-list shape as the impact report: header per name, one line
/// per declaration site.
pub fn dupes_report(groups: &[DupeGroup]) -> String {
    let mut out = String::new();
    for (i, group) in groups.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format!("{} — {} repos\n", group.name, group.repo_count));
        for site in &group.sites {
            out.push_str(&format!("  {} · {}:{}\n", site.repo, site.file, site.line));
        }
    }
    out
}

pub fn clones_json(out: &ClonesOutput) -> Result<String> {
    Ok(serde_json::to_string_pretty(out)?)
}

/// One block per cluster: numbered header, aligned member lines, `→` notes;
/// npm blind-spot footer at the end. Same grouped-list family as the impact
/// and dupes reports.
pub fn clones_report(out: &ClonesOutput) -> String {
    let mut s = String::new();
    for (i, cluster) in out.clusters.iter().enumerate() {
        if i > 0 {
            s.push('\n');
        }
        s.push_str(&format!(
            "CLONE CLUSTER #{} — {} members · sig {} · {} tokens\n",
            i + 1,
            cluster.members.len(),
            cluster.sig,
            cluster.token_count
        ));
        let loc = |m: &crate::features::clones::CloneSite| format!("{} · {}:{}", m.repo, m.file, m.line);
        let width = cluster
            .members
            .iter()
            .map(|m| loc(m).len())
            .max()
            .unwrap_or(0);
        for m in &cluster.members {
            let suffix = if m.exported { "" } else { " (not exported)" };
            s.push_str(&format!("  {:<width$}   {}{}\n", loc(m), m.name, suffix));
        }
        if !cluster.literals.is_empty() {
            s.push_str(&format!(
                "  → identical body · literals: {}\n",
                cluster.literals.join(" ")
            ));
        }
        for note in &cluster.notes {
            s.push_str(&format!("  → {note}\n"));
        }
    }
    if !out.npm_only_packages.is_empty() {
        if !out.clusters.is_empty() {
            s.push('\n');
        }
        s.push_str(&format!(
            "note: {} lxp-common-* package(s) are npm-only (not scanned); body-clone detection skipped for them: {}\n",
            out.npm_only_packages.len(),
            out.npm_only_packages.join(", ")
        ));
    }
    s
}

pub fn context_json(pack: &ContextPack) -> Result<String> {
    Ok(serde_json::to_string_pretty(pack)?)
}

/// TTY gets bat-style highlighted excerpts; piped output stays plain so the
/// pack can be pasted into a task brief unchanged.
fn maybe_highlight(code: &str) -> String {
    if std::io::stdout().is_terminal() {
        crate::output::highlight::highlight_ansi(code)
    } else {
        code.to_string()
    }
}

/// LLM-ready markdown pack: definition, prop frequencies, usage excerpts.
pub fn context_markdown(pack: &ContextPack, root_display: &str) -> String {
    let mut out = format!(
        "# Context: {}\n\nScanned {root_display} · {} sites · {} files · {} repos\n",
        pack.symbol, pack.total_sites, pack.total_files, pack.total_repos
    );

    out.push_str("\n## Definition\n");
    match &pack.definition {
        Some(def) => {
            out.push_str(&format!(
                "{}/{}:{}\n```tsx\n{}```\n",
                def.repo,
                def.file,
                def.line,
                maybe_highlight(&def.excerpt)
            ));
        }
        None => out.push_str("not located (no top-level declaration found in the workspace)\n"),
    }

    if !pack.prop_counts.is_empty() {
        out.push_str("\n## Props observed across usages\n");
        let freq: Vec<String> = pack
            .prop_counts
            .iter()
            .map(|(prop, count)| format!("{prop} ×{count}"))
            .collect();
        out.push_str(&freq.join(" · "));
        out.push('\n');
    }

    out.push_str(&format!(
        "\n## Usage excerpts ({} of {} sites)\n",
        pack.excerpts.len(),
        pack.total_sites
    ));
    for excerpt in &pack.excerpts {
        out.push_str(&format!(
            "### {} · {}:{}\n```tsx\n{}\n```\n",
            excerpt.repo,
            excerpt.file,
            excerpt.line,
            maybe_highlight(&excerpt.code)
        ));
    }

    if !pack.same_name.is_empty() {
        out.push_str(&format!(
            "\n## Other components named {} (NOT in this pack)\n",
            pack.symbol
        ));
        for group in &pack.same_name {
            out.push_str(&format!(
                "- {} — {} site(s) · repack with `--from \"{}\"`\n",
                group.repo, group.sites, group.from_hint
            ));
        }
    }
    out
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
    use crate::scan::discover::Repo;
    use crate::features::drift::compute_drift;
    use crate::features::impact::ImpactHit;
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
        vec![
            ImpactHit {
                repo: "app-one".to_string(),
                file: "src/page.tsx".to_string(),
                line: 1,
                source: "fake-lib/components/Button".to_string(),
                refs: 0,
                jsx_uses: 1,
                jsx_props: ["variant", "size"].iter().map(|s| s.to_string()).collect(),
                jsx_lines: vec![1],
            },
            ImpactHit {
                repo: "app-one".to_string(),
                file: "src/util.ts".to_string(),
                line: 3,
                source: "fake-lib/components/Button".to_string(),
                refs: 2,
                jsx_uses: 0,
                jsx_props: Default::default(),
                jsx_lines: Vec::new(),
            },
            ImpactHit {
                repo: "app-two".to_string(),
                file: "src/other.tsx".to_string(),
                line: 7,
                source: "fake-lib/components/Button".to_string(),
                refs: 0,
                jsx_uses: 0,
                jsx_props: Default::default(),
                jsx_lines: Vec::new(),
            },
        ]
    }

    #[test]
    fn impact_json_roundtrips() {
        let json = impact_json(&sample_hits()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v[0]["repo"], "app-one");
        assert_eq!(v[0]["jsx_props"][0], "size");
    }

    #[test]
    fn impact_report_groups_hits_under_one_header_per_repo() {
        let report = impact_report(&sample_hits());
        let lines: Vec<&str> = report.lines().collect();
        assert_eq!(lines[0], "app-one (2 sites)");
        assert_eq!(
            report.lines().filter(|l| l.starts_with("app-one")).count(),
            1
        );
        assert!(report.contains("\napp-two (1 site)\n"));
    }

    #[test]
    fn impact_report_hit_lines_carry_location_counts_source_and_props() {
        let report = impact_report(&sample_hits());
        assert!(report.contains("  src/page.tsx:1\n"));
        assert!(report.contains("jsx ×1"));
        assert!(report.contains("props: size, variant"));
        assert!(report.contains("ref ×2"));
        assert!(report.contains("from fake-lib/components/Button"));
        // import with no refs/jsx must still say something, not render an empty line
        assert!(report.contains("import only"));
    }

    #[test]
    fn impact_report_on_no_hits_is_empty() {
        assert_eq!(impact_report(&[]), "");
    }

    fn sample_clones() -> ClonesOutput {
        use crate::features::clones::{CloneCluster, CloneSite};
        use crate::scan::fingerprint::CandidateKind;
        ClonesOutput {
            clusters: vec![CloneCluster {
                members: vec![
                    CloneSite {
                        repo: "app-one".to_string(),
                        file: "src/utils/validators.ts".to_string(),
                        line: 3,
                        name: "isEmail".to_string(),
                        exported: true,
                        kind: CandidateKind::Const,
                        sig: "(email: string)".to_string(),
                    },
                    CloneSite {
                        repo: "app-two".to_string(),
                        file: "src/utils/check.ts".to_string(),
                        line: 2,
                        name: "validateEmail".to_string(),
                        exported: false,
                        kind: CandidateKind::Fn,
                        sig: "(value: string)".to_string(),
                    },
                ],
                token_count: 24,
                sig: "(email: string)".to_string(),
                literals: vec![r"/^[^\s@]+@[^\s@]+\.[^\s@]+$/".to_string()],
                notes: vec![
                    "no isEmail/validateEmail export found in lxp-common-functions-js — candidate shared home"
                        .to_string(),
                ],
            }],
            npm_only_packages: vec![
                "lxp-common-components-js".to_string(),
                "lxp-common-functions-js".to_string(),
            ],
        }
    }

    #[test]
    fn clones_report_shows_header_aligned_members_literals_and_notes() {
        let report = clones_report(&sample_clones());
        let lines: Vec<&str> = report.lines().collect();
        assert_eq!(
            lines[0],
            "CLONE CLUSTER #1 — 2 members · sig (email: string) · 24 tokens"
        );
        assert!(report.contains("  app-one · src/utils/validators.ts:3"));
        assert!(report.contains("isEmail\n"));
        assert!(
            report.contains("validateEmail (not exported)"),
            "unexported members are flagged: {report}"
        );
        assert!(report.contains(r"literals: /^[^\s@]+@[^\s@]+\.[^\s@]+$/"));
        assert!(report.contains("  → no isEmail/validateEmail export found"));
        assert!(report.contains(
            "note: 2 lxp-common-* package(s) are npm-only (not scanned); body-clone detection skipped for them: lxp-common-components-js, lxp-common-functions-js"
        ));
        // member name column is aligned across the cluster
        let member_lines: Vec<&str> = lines
            .iter()
            .filter(|l| l.contains(" · src/"))
            .copied()
            .collect();
        let name_cols: Vec<usize> = member_lines
            .iter()
            .map(|l| {
                l.find("isEmail")
                    .or_else(|| l.find("validateEmail"))
                    .unwrap()
            })
            .collect();
        assert_eq!(name_cols[0], name_cols[1], "aligned: {member_lines:?}");
    }

    #[test]
    fn clones_report_with_no_clusters_still_prints_the_npm_footer() {
        let out = ClonesOutput {
            clusters: vec![],
            npm_only_packages: vec!["lxp-common-functions-js".to_string()],
        };
        let report = clones_report(&out);
        assert!(report.starts_with("note: 1 lxp-common-* package(s)"));
    }

    #[test]
    fn clones_report_on_fully_empty_output_is_empty() {
        let out = ClonesOutput {
            clusters: vec![],
            npm_only_packages: vec![],
        };
        assert_eq!(clones_report(&out), "");
    }

    #[test]
    fn clones_json_roundtrips() {
        let json = clones_json(&sample_clones()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["clusters"][0]["members"][0]["name"], "isEmail");
        assert_eq!(v["clusters"][0]["members"][1]["kind"], "fn");
        assert_eq!(v["clusters"][0]["token_count"], 24);
        assert_eq!(v["npm_only_packages"][1], "lxp-common-functions-js");
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
