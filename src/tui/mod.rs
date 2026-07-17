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
    let repos = crate::scan::discover::discover_repos(root, &mut warnings)?;
    let exports = crate::features::dupes::scan_exports(root, &mut warnings)?;
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
                // keep the target visible so the user can open it by hand
                app.status = Some(format!("{err:#} — {}:{line}", path.display()));
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
    tx: mpsc::Sender<(String, Result<crate::features::context::ContextPack, String>)>,
    root: PathBuf,
    symbol: String,
) {
    std::thread::spawn(move || {
        let mut warnings = Vec::new();
        let result = crate::features::context::build_context(&root, &symbol, None, 5, &mut warnings)
            .map_err(|err| format!("{err:#}"));
        // receiver gone = TUI already closed; nothing to do
        let _ = tx.send((symbol, result));
    });
}

/// Terminal editors need the screen: suspend the TUI, run `$EDITOR +line`,
/// resume. Without $EDITOR fall back to a GUI editor, which opens detached.
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
            Ok(())
        }
        _ => open_gui_editor(path, line),
    }
}

/// `code`/`cursor` CLIs when installed; otherwise the vscode:// URL scheme
/// via `open`, which reaches VS Code even when its shell command was never
/// installed. Only spawn what actually exists — a blind spawn dies with a
/// bare "os error 2" and no hint.
fn open_gui_editor(path: &Path, line: usize) -> anyhow::Result<()> {
    for cli in ["code", "cursor"] {
        if find_in_path(cli).is_some() {
            Command::new(cli)
                .arg("-g")
                .arg(format!("{}:{line}", path.display()))
                .spawn()
                .with_context(|| format!("launching `{cli}`"))?;
            return Ok(());
        }
    }
    if cfg!(target_os = "macos")
        && let Some(url) = editor_url(path, line)
    {
        let status = Command::new("open").arg(&url).status().context("running `open`")?;
        anyhow::ensure!(status.success(), "`open {url}` failed");
        return Ok(());
    }
    anyhow::bail!("no editor found — set $EDITOR or install the `code`/`cursor` CLI")
}

/// file:line URL for whichever VS Code-family app is actually installed —
/// their URL schemes work even when the shell command was never set up.
fn editor_url(path: &Path, line: usize) -> Option<String> {
    let apps = [
        ("Visual Studio Code.app", "vscode"),
        ("Cursor.app", "cursor"),
    ];
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let scheme = apps.iter().find_map(|(app, scheme)| {
        let in_root = Path::new("/Applications").join(app).exists();
        let in_home = home
            .as_ref()
            .is_some_and(|h| h.join("Applications").join(app).exists());
        (in_root || in_home).then_some(*scheme)
    })?;
    let absolute = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    Some(format!("{scheme}://file{}:{line}", absolute.display()))
}

fn find_in_path(program: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}
