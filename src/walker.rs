use std::path::{Path, PathBuf};

const EXTENSIONS: [&str; 4] = ["ts", "tsx", "js", "jsx"];
const SKIP_DIRS: [&str; 4] = ["node_modules", "build", "dist", "coverage"];

pub fn source_files(repo_root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let walker = ignore::WalkBuilder::new(repo_root)
        .hidden(true)
        .git_ignore(true)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !SKIP_DIRS.contains(&name.as_ref())
        })
        .build();
    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !EXTENSIONS.contains(&ext) {
            continue;
        }
        if path.to_string_lossy().ends_with(".d.ts") {
            continue;
        }
        out.push(path.to_path_buf());
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_one() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/workspace/app-one")
    }

    #[test]
    fn includes_ts_tsx_and_skips_node_modules_md_and_dts() {
        let files = source_files(&app_one());
        let names: Vec<String> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"page.tsx".to_string()));
        assert!(names.contains(&"helpers.ts".to_string()));
        assert!(!names.iter().any(|n| n == "x.ts")); // node_modules
        assert!(!names.iter().any(|n| n == "readme.md"));
        assert!(!names.iter().any(|n| n == "globals.d.ts"));
    }
}
