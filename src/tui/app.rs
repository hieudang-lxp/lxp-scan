use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use crate::context::ContextPack;
use crate::dupes::DeclSite;
use crate::tui::fuzzy;

pub struct SymbolEntry {
    pub name: String,
    /// declaration sites, used for the repo tags and as the jump target
    /// before a context pack is loaded
    pub sites: Vec<DeclSite>,
}

impl SymbolEntry {
    pub fn repos(&self) -> Vec<&str> {
        let mut repos: Vec<&str> = self.sites.iter().map(|s| s.repo.as_str()).collect();
        repos.sort_unstable();
        repos.dedup();
        repos
    }
}

pub enum PackState {
    Loading,
    Ready(Box<ContextPack>),
    Failed(String),
}

/// What the event loop must do next; produced by pure key handling so the
/// state machine stays testable without a terminal.
#[derive(Debug, PartialEq)]
pub enum Action {
    OpenEditor(PathBuf, usize),
}

pub struct App {
    pub symbols: Vec<SymbolEntry>,
    pub repo_roots: HashMap<String, PathBuf>,
    pub filter: String,
    /// indices into `symbols`, best match first
    pub filtered: Vec<usize>,
    /// index into `filtered`
    pub selected: usize,
    pub packs: HashMap<String, PackState>,
    /// which usage excerpt of the selected symbol is shown
    pub excerpt_idx: usize,
    pub scan_warnings: usize,
    pub status: Option<String>,
    pub should_quit: bool,
}

impl App {
    pub fn new(
        exports: BTreeMap<String, Vec<DeclSite>>,
        repo_roots: HashMap<String, PathBuf>,
        scan_warnings: usize,
    ) -> Self {
        // BTreeMap iteration keeps the symbol list name-sorted
        let symbols = exports
            .into_iter()
            .map(|(name, sites)| SymbolEntry { name, sites })
            .collect();
        let mut app = App {
            symbols,
            repo_roots,
            filter: String::new(),
            filtered: Vec::new(),
            selected: 0,
            packs: HashMap::new(),
            excerpt_idx: 0,
            scan_warnings,
            status: None,
            should_quit: false,
        };
        app.refilter();
        app
    }

    fn refilter(&mut self) {
        let mut scored: Vec<(i32, usize)> = self
            .symbols
            .iter()
            .enumerate()
            .filter_map(|(idx, sym)| fuzzy::score(&self.filter, &sym.name).map(|s| (s, idx)))
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        self.filtered = scored.into_iter().map(|(_, idx)| idx).collect();
        self.selected = 0;
        self.excerpt_idx = 0;
    }

    pub fn push_filter(&mut self, c: char) {
        self.filter.push(c);
        self.refilter();
    }

    pub fn pop_filter(&mut self) {
        self.filter.pop();
        self.refilter();
    }

    pub fn clear_filter(&mut self) {
        self.filter.clear();
        self.refilter();
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.filtered.len() {
            self.selected += 1;
            self.excerpt_idx = 0;
        }
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            self.excerpt_idx = 0;
        }
    }

    pub fn selected_symbol(&self) -> Option<&SymbolEntry> {
        self.filtered.get(self.selected).map(|&i| &self.symbols[i])
    }

    pub fn current_pack(&self) -> Option<&PackState> {
        self.packs.get(&self.selected_symbol()?.name)
    }

    fn excerpt_count(&self) -> usize {
        match self.current_pack() {
            Some(PackState::Ready(pack)) => pack.excerpts.len(),
            _ => 0,
        }
    }

    pub fn next_excerpt(&mut self) {
        let count = self.excerpt_count();
        if count > 0 {
            self.excerpt_idx = (self.excerpt_idx + 1) % count;
        }
    }

    pub fn prev_excerpt(&mut self) {
        let count = self.excerpt_count();
        if count > 0 {
            self.excerpt_idx = (self.excerpt_idx + count - 1) % count;
        }
    }

    /// Symbol whose context pack should be scanned now, if any.
    pub fn needs_scan(&self) -> Option<String> {
        let name = &self.selected_symbol()?.name;
        (!self.packs.contains_key(name)).then(|| name.clone())
    }

    pub fn on_scan_result(&mut self, symbol: String, result: Result<ContextPack, String>) {
        let state = match result {
            Ok(pack) => PackState::Ready(Box::new(pack)),
            Err(err) => PackState::Failed(err),
        };
        self.packs.insert(symbol, state);
    }

    /// Jump target: the shown usage excerpt when the pack is loaded,
    /// otherwise the symbol's first declaration site.
    pub fn open_target(&self) -> Option<Action> {
        let symbol = self.selected_symbol()?;
        if let Some(PackState::Ready(pack)) = self.current_pack()
            && let Some(excerpt) = pack.excerpts.get(self.excerpt_idx)
        {
            let root = self.repo_roots.get(&excerpt.repo)?;
            return Some(Action::OpenEditor(root.join(&excerpt.file), excerpt.line));
        }
        let site = symbol.sites.first()?;
        let root = self.repo_roots.get(&site.repo)?;
        Some(Action::OpenEditor(root.join(&site.file), site.line))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::UsageExcerpt;

    fn site(repo: &str, file: &str, line: usize) -> DeclSite {
        DeclSite {
            repo: repo.to_string(),
            file: file.to_string(),
            line,
        }
    }

    fn app() -> App {
        let mut exports = BTreeMap::new();
        exports.insert("Button".to_string(), vec![site("lib", "src/Button.tsx", 3)]);
        exports.insert(
            "ButtonGroup".to_string(),
            vec![site("lib", "src/ButtonGroup.tsx", 1)],
        );
        exports.insert("Card".to_string(), vec![site("app", "src/Card.tsx", 9)]);
        let roots = HashMap::from([
            ("lib".to_string(), PathBuf::from("/ws/lib")),
            ("app".to_string(), PathBuf::from("/ws/app")),
        ]);
        App::new(exports, roots, 0)
    }

    fn ready_pack(app: &mut App, symbol: &str, excerpts: Vec<UsageExcerpt>) {
        let pack = ContextPack {
            symbol: symbol.to_string(),
            total_sites: excerpts.len(),
            total_files: excerpts.len(),
            total_repos: 1,
            prop_counts: vec![],
            definition: None,
            excerpts,
            same_name: vec![],
        };
        app.on_scan_result(symbol.to_string(), Ok(pack));
    }

    fn excerpt(repo: &str, file: &str, line: usize) -> UsageExcerpt {
        UsageExcerpt {
            repo: repo.to_string(),
            file: file.to_string(),
            line,
            jsx_props: Default::default(),
            code: "<Button />".to_string(),
        }
    }

    #[test]
    fn filter_narrows_and_ranks_prefix_first() {
        let mut app = app();
        assert_eq!(app.filtered.len(), 3);
        for c in "but".chars() {
            app.push_filter(c);
        }
        let names: Vec<&str> = app
            .filtered
            .iter()
            .map(|&i| app.symbols[i].name.as_str())
            .collect();
        assert_eq!(names, vec!["Button", "ButtonGroup"]);
        app.clear_filter();
        assert_eq!(app.filtered.len(), 3);
    }

    #[test]
    fn selection_clamps_and_resets_excerpt_index() {
        let mut app = app();
        app.excerpt_idx = 2;
        app.move_down();
        assert_eq!(app.selected, 1);
        assert_eq!(app.excerpt_idx, 0);
        app.move_down();
        app.move_down(); // clamped at last entry
        assert_eq!(app.selected, 2);
        app.move_up();
        app.move_up();
        app.move_up(); // clamped at first entry
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn needs_scan_only_for_unloaded_symbols() {
        let mut app = app();
        assert_eq!(app.needs_scan().as_deref(), Some("Button"));
        app.packs.insert("Button".to_string(), PackState::Loading);
        assert_eq!(app.needs_scan(), None);
        app.on_scan_result("Button".to_string(), Err("boom".to_string()));
        assert!(matches!(
            app.current_pack(),
            Some(PackState::Failed(msg)) if msg == "boom"
        ));
        assert_eq!(app.needs_scan(), None);
    }

    #[test]
    fn excerpt_cycling_wraps_and_open_targets_the_shown_site() {
        let mut app = app();
        // before the pack loads, Enter jumps to the declaration
        assert_eq!(
            app.open_target(),
            Some(Action::OpenEditor(PathBuf::from("/ws/lib/src/Button.tsx"), 3))
        );
        ready_pack(
            &mut app,
            "Button",
            vec![excerpt("app", "src/a.tsx", 10), excerpt("app", "src/b.tsx", 20)],
        );
        app.next_excerpt();
        assert_eq!(app.excerpt_idx, 1);
        assert_eq!(
            app.open_target(),
            Some(Action::OpenEditor(PathBuf::from("/ws/app/src/b.tsx"), 20))
        );
        app.next_excerpt();
        assert_eq!(app.excerpt_idx, 0, "wraps around");
        app.prev_excerpt();
        assert_eq!(app.excerpt_idx, 1);
    }

    #[test]
    fn repos_are_deduped_per_symbol() {
        let entry = SymbolEntry {
            name: "X".to_string(),
            sites: vec![site("a", "1.tsx", 1), site("a", "2.tsx", 1), site("b", "3.tsx", 1)],
        };
        assert_eq!(entry.repos(), vec!["a", "b"]);
    }
}
