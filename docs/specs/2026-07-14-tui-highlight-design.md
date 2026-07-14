# Design: syntax-highlighted excerpts + `lxp-scan tui`

Date: 2026-07-14 · Status: approved

## Goal

Make lxp-scan demo-worthy for the team by borrowing the two UX patterns that
make 2026's top Rust CLIs (bat, gitui, yazi) stick:

1. **Syntax-highlighted code excerpts** in `context` output (bat-style).
2. **`lxp-scan tui`** — an interactive component explorer (gitui-style).

## Feature A: highlighted excerpts

- New module `src/highlight.rs`:
  - `highlight_ansi(code: &str) -> String` — TSX-highlighted, 24-bit ANSI.
  - Engine: `syntect` with `two-face` syntax set (bat's bundled syntaxes —
    stock syntect has no TSX). SyntaxSet/Theme in a `OnceLock`.
- Wiring: `report::context_markdown` highlights the Definition and Usage
  excerpt code blocks **only when stdout is a TTY** (existing convention).
  Piped output and `--json` stay byte-identical to today — context packs are
  pasted into task briefs and must remain plain.

## Feature B: `lxp-scan tui`

New subcommand `tui { --root }`. Deps: `ratatui`, `crossterm`.

### Layout

```
┌ Components ──────────┬ Button — fake-lib ─────────────────────────┐
│ > But█               │ 37 sites · 12 files · 4 repos              │
│   Button             │ props: size ×21, onClick ×18, …            │
│   ButtonGroup        │ ── Definition (fake-lib/src/Button.tsx) ── │
│   BackButton         │ <highlighted code>                         │
│                      │ ── Usage 2/5 (lxp-web/src/Login.tsx:52) ── │
│                      │ <highlighted code>                         │
└ type to filter ──────┴ ↑↓ select · Tab site · Enter open · Esc ───┘
```

### Modules

| File | Responsibility |
|---|---|
| `tui/mod.rs` | terminal setup/teardown, event loop, background-scan channel, editor spawn |
| `tui/app.rs` | pure state: symbol list, filter, selection, `HashMap<String, PackState>` cache (`Loading`/`Ready`), excerpt index. Unit-tested; no terminal deps |
| `tui/fuzzy.rs` | hand-rolled scorer: prefix > substring > subsequence, case-insensitive. Unit-tested |
| `tui/ui.rs` | ratatui render; syntect spans → ratatui `Line`s |

### Data flow

- Startup: `discover_repos` + the dupes export scan generalized to **all**
  exported component-shaped names (Task: extract `scan_exports` shared by
  `dupes` and the TUI). Deduped names, defining repos attached.
- Selection settles (debounce ~200ms) → spawn thread running
  `context::build_context(root, symbol, None, 5)` → send over `mpsc` →
  cache. UI shows `loading…` meanwhile; results are cached per symbol.
- Enter → suspend terminal, `$EDITOR +line file` (absolute path via the
  repo root); if `$EDITOR` unset, `code -g file:line`; resume.

### Keys

Printable = filter · Backspace · ↑/↓ select · Tab/Shift-Tab cycle excerpts ·
Enter open editor · Esc clear filter, or quit when empty · Ctrl-C quit.

## Error handling

House rules apply: per-item failures become warnings (TUI shows a warning
count in the footer; `--verbose` not applicable). Background scan errors
render as a message in the right pane, never crash the loop.

## Testing

- `fuzzy.rs`, `app.rs`: unit tests (filtering, selection movement, cache
  transitions, site cycling).
- `highlight.rs`: output contains ANSI escapes and the input text; piped
  `context_markdown` stays plain (existing integration tests must not break).
- Manual smoke on `~/Leapxpert/FE`.

## Out of scope (YAGNI)

Frecency ranking, dupes-only toggle, watch mode, delta-style props diff,
Windows terminal support beyond what crossterm gives for free.
