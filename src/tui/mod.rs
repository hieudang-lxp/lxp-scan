pub mod app;
pub mod fuzzy;
mod ui;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

use app::{Action, App, PackState};

/// Selection must settle this long before a context scan is kicked off, so
/// holding an arrow key doesn't queue a scan per row.
const SCAN_DEBOUNCE: Duration = Duration::from_millis(150);
const POLL_INTERVAL: Duration = Duration::from_millis(100);

pub fn run(root: &Path) -> anyhow::Result<()> {
    let mut warnings = Vec::new();
    eprintln!("scanning {} …", root.display());
    let repos = crate::discover::discover_repos(root, &mut warnings)?;
    let exports = crate::dupes::scan_exports(root, &mut warnings)?;
    if exports.is_empty() {
        anyhow::bail!(
            "no exported components found under {} — is --root pointing at the FE workspace?",
            root.display()
        );
    }
    let repo_roots = repos
        .into_iter()
        .map(|r| (r.name, r.root))
        .collect::<std::collections::HashMap<_, _>>();
    let mut app = App::new(exports, repo_roots, warnings.len());

    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &mut app, root);
    ratatui::restore();
    result
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    root: &Path,
) -> anyhow::Result<()> {
    let (tx, rx) = mpsc::channel();
    let mut last_input = Instant::now();
    loop {
        if app.should_quit {
            return Ok(());
        }
        if last_input.elapsed() >= SCAN_DEBOUNCE
            && let Some(symbol) = app.needs_scan()
        {
            app.packs.insert(symbol.clone(), PackState::Loading);
            spawn_scan(tx.clone(), root.to_path_buf(), symbol);
        }
        while let Ok((symbol, result)) = rx.try_recv() {
            app.on_scan_result(symbol, result);
        }
        terminal.draw(|frame| ui::render(frame, app))?;
        if !event::poll(POLL_INTERVAL)? {
            continue;
        }
        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            last_input = Instant::now();
            let action = handle_key(app, key.code, key.modifiers);
            if let Some(Action::OpenEditor(path, line)) = action
                && let Err(err) = open_editor(terminal, &path, line)
            {
                app.status = Some(format!("open failed: {err:#}"));
            }
        }
    }
}

fn handle_key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> Option<Action> {
    app.status = None;
    match code {
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
            app.should_quit = true;
        }
        KeyCode::Esc => {
            if app.filter.is_empty() {
                app.should_quit = true;
            } else {
                app.clear_filter();
            }
        }
        KeyCode::Up => app.move_up(),
        KeyCode::Down => app.move_down(),
        KeyCode::Tab => app.next_excerpt(),
        KeyCode::BackTab => app.prev_excerpt(),
        KeyCode::Backspace => app.pop_filter(),
        KeyCode::Enter => return app.open_target(),
        KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => app.push_filter(c),
        _ => {}
    }
    None
}

fn spawn_scan(
    tx: mpsc::Sender<(String, Result<crate::context::ContextPack, String>)>,
    root: PathBuf,
    symbol: String,
) {
    std::thread::spawn(move || {
        let mut warnings = Vec::new();
        let result = crate::context::build_context(&root, &symbol, None, 5, &mut warnings)
            .map_err(|err| format!("{err:#}"));
        // receiver gone = TUI already closed; nothing to do
        let _ = tx.send((symbol, result));
    });
}

/// Terminal editors need the screen: suspend the TUI, run `$EDITOR +line`,
/// resume. Without $EDITOR fall back to VS Code's `code -g file:line`,
/// which opens detached.
fn open_editor(terminal: &mut DefaultTerminal, path: &Path, line: usize) -> anyhow::Result<()> {
    match std::env::var("EDITOR") {
        Ok(editor) if !editor.trim().is_empty() => {
            let mut parts = editor.split_whitespace();
            let program = parts.next().expect("non-empty by guard");
            let args: Vec<&str> = parts.collect();
            ratatui::restore();
            let status = Command::new(program)
                .args(args)
                .arg(format!("+{line}"))
                .arg(path)
                .status();
            *terminal = ratatui::init();
            let status = status.with_context(|| format!("running $EDITOR ({program})"))?;
            anyhow::ensure!(status.success(), "$EDITOR exited with {status}");
        }
        _ => {
            Command::new("code")
                .arg("-g")
                .arg(format!("{}:{line}", path.display()))
                .spawn()
                .context("launching `code` (set $EDITOR for a terminal editor)")?;
        }
    }
    Ok(())
}
