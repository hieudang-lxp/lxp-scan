# lxp-scan

Cross-repo intelligence CLI for the LeapXpert FE tree:

- **`impact`** — where is a symbol imported and used, with which props?
- **`context`** — LLM-ready context pack for a symbol: definition + usage excerpts
- **`dupes`** — same-name components implemented independently in multiple repos
- **`clones`** — same-body functions under *different* names, across repos (name-agnostic)
- **`drift`** — which repos are on diverging versions of `lxp-common-*` packages?
- **`tui`** — interactive component explorer: fuzzy-find, browse usages, jump to editor
- **`mcp`** — stdio MCP server exposing all of the above to coding agents

AST-based (oxc), tsconfig-alias-aware, parallel. Scans ~4,400 files in ~1s.

## Quick start

```bash
cd ~/Leapxpert/FE
lxp-scan drift
```

```
| package                  | cic-admin-web | lxp-app-admin | lxp-web | lxp-web-client | drift |
| lxp-common-components-js | ^2.1.56       | ^3.1.32       | ^3.1.25 | ^2.0.64        | Major |
```

> `command not found: lxp-scan` → run `rehash` or open a new tab. The binary is
> a symlink at `~/.local/bin/lxp-scan` pointing into `target/release`.

## impact

```bash
lxp-scan impact Button --from lxp-common-components-js
```

```
lxp-web (12 sites)
  src/.../LoginForm.tsx:5
      jsx ×1 · from lxp-common-components-js/components/Button · props: className, disabled

12 usage site(s) in 12 file(s) across 1 repo(s)
```

One header per repo, two lines per usage site:

| Part | Meaning |
|---|---|
| `file:line` | Import statement location, relative to the repo |
| `ref ×N` | Uses as a value/function/type (excludes JSX tags) |
| `jsx ×N` | Times rendered as `<Symbol ...>` |
| `from` | Resolved import source — packages verbatim; intra-repo files prefixed with the repo name (alias and relative imports of the same file display identically) |
| `props` | Union of props passed across all renders in the file |

Common names (`Button`, `Modal`) collide across repos — always pass `--from`.
Intra-repo utils work too (aliases resolved): `--from utils/formatMessage`.

## context

```bash
lxp-scan context Avatar --from lxp-common-components-js --root ~/Leapxpert/FE
```

A markdown pack on stdout, ready to paste into a task brief:

- header with totals (sites / files / repos)
- **Definition** — the real declaration behind barrel re-exports, plus its
  `XxxProps` / `IXxxProps` interface (each capped at 30 lines)
- **Props observed across usages** — per-prop site counts, e.g. `profile ×45`
- **Usage excerpts** — up to `--sites` (default 8) representative sites, anchored
  at the JSX render line, round-robined across repos, preferring unseen prop sets

Same-name components never blend into one pack: hits are grouped by the repo
defining the imported component, the pack covers the dominant group, and an
"Other components named X" section lists the rest with a `--from` hint to repack
them. `--json` emits the full pack as JSON.

On a TTY, `context` excerpts are syntax-highlighted (bat-style); piped output
stays plain so packs paste cleanly.

## tui

```bash
lxp-scan tui --root ~/Leapxpert/FE
```

Full-screen explorer over every exported component. Type to fuzzy-filter;
selecting a symbol runs the context scan in the background (cached) and shows
totals, prop frequencies, the current usage excerpt, and the definition — all
syntax-highlighted.

| Key | Action |
|---|---|
| type / Backspace | fuzzy-filter the component list |
| ↑ / ↓ | select a component |
| Tab / Shift-Tab | cycle usage excerpts |
| Enter | open the shown site in `$EDITOR` (`+line`), or `code -g` |
| Esc | clear the filter, or quit when it's empty |

## dupes

```bash
lxp-scan dupes --root ~/Leapxpert/FE
```

Lists capitalized exported values (`const`/`function`/`class`, excluding
`*Props`, tests, and stories) declared in **more than one repo** — parallel
implementations that are candidates for consolidation into lxp-common:

```
ConfirmPopup — 2 repos
  lxp-app-admin · src/components/ConfirmPopup/index.tsx:33
  lxp-web · src/components/ConfirmPopup/index.tsx:41
```

## clones

```bash
lxp-scan clones --root ~/Leapxpert/FE
```

`dupes` matches exported component **names**; `clones` matches function
**bodies**. Every top-level `function f() {}` / `const f = () => {}` is
fingerprinted: identifiers normalized away, comments/whitespace dropped,
string/regex/number literals kept verbatim — so `isEmail` and `validateEmail`
cluster when their bodies match, while validators with the same shape but
different regexes never do.

```
CLONE CLUSTER #2 — 2 members · sig (email: string) · 20 tokens
  lxp-app-admin · src/utils/validators.ts:3    isEmail
  lxp-web       · src/utils/check.ts:2         validateEmail
  → identical body · literals: /^[^\s@]+@[^\s@]+\.[^\s@]+$/
  → no isEmail/validateEmail export found in lxp-common-functions-js — candidate shared home
```

Cluster notes cross-check candidate names against the exports of npm-only
`lxp-common-*` packages (harvested from their `.d.ts` files) — either the util
already exists in the shared home, or the cluster is a candidate to move there.

Flags: `--symbol <name>`, `--min-tokens <n>` (floor, default 10; lower to catch
one-liners), `--same-file`, `--kind fn|const|all`.

**Scope (v1):** exact structural clones only — bodies must be identical after
normalization. Reimplementations with extra guards or a different algorithm are
not found. Class methods, object-literal methods, nested closures and wrapped
declarations (`memo(() => ...)`) are not candidates.

## Claude Code plugin (team install)

No Rust toolchain needed — two commands:

```bash
claude plugin marketplace add hieudang-lxp/lxp-scan
claude plugin install lxp-scan@lxp-tools
```

This gives Claude Code five MCP tools (`impact`, `context`, `drift`, `dupes`,
`clones`) plus a skill teaching the agent when to use them. On first use the
plugin downloads the darwin-arm64 binary from GitHub Releases (cached in
`~/.cache/lxp-scan`) and links it to `~/.local/bin/lxp-scan`, so the full CLI —
including `lxp-scan tui` — works from any terminal too. Intel Macs / Linux build
from source.

**Workspace root detection:** when Claude Code runs inside a repo (a dir with
`package.json`), the parent directory is scanned — so sibling FE repos are
visible. Override with `export LXP_SCAN_ROOT=/path/to/fe-workspace`.

**Manual registration** (locally built binary, without the plugin):

```bash
claude mcp add --scope user lxp-scan -- ~/.local/bin/lxp-scan mcp --root ~/Leapxpert/FE
```

## Flags

| Flag | Default | Description |
|---|---|---|
| `--root <dir>` | `.` | Directory of repos (repo = subdir with a package.json) |
| `--from <substring>` | — | (impact/context) Filter by resolved import source |
| `--sites <n>` | `8` | (context) Maximum usage excerpts |
| `--symbol <name>` | — | (clones) Only clusters containing this declaration name |
| `--min-tokens <n>` | `10` | (clones) Minimum normalized body tokens |
| `--kind fn\|const\|all` | `all` | (clones) Declaration form to scan |
| `--same-file` | off | (clones) Also report clusters within one file |
| `--json` | text | JSON on stdout |
| `--verbose` | off | Warnings on stderr |

Exit code 0 even with zero matches; 1 on errors. Tables/JSON on stdout,
warnings and summary on stderr.

## Known v1 limitations

- Not followed: namespace imports (`import * as X`), re-export chains,
  dynamic `import()`, `require()`.
- Type-position references count toward `refs`.
- `<Button.Icon />` counts as a `ref` of `Button`, not `jsx`.
- `drift` ignores patch-level differences and skips unparseable versions
  (`workspace:*`, `latest`, git URLs).
- Hidden directories and tsconfig `extends` chains are skipped.

## Troubleshooting

| Problem | Fix |
|---|---|
| `command not found: lxp-scan` | `rehash`, or open a new terminal |
| Stale output after rebuild | `which -a lxp-scan` must list only `~/.local/bin/lxp-scan` (do NOT `cargo install --path .`) |
| Empty output but usage exists | Check `--from`; try without it; add `--verbose` for parse failures |
| Results differ from grep | See limitations — usually re-exports/namespace imports (skipped) or multi-line imports (grep misses them) |

## Development

```bash
cd ~/Leapxpert/tools/lxp-scan
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release   # ~/.local/bin/lxp-scan symlinks here — no install step
cargo run --release -- drift --root ~/Leapxpert/FE
```

Source layout: `scan/` (discovery, resolution, AST analysis) · `features/` (one
orchestrator per subcommand) · `output/` (rendering + highlighting) · `mcp.rs`,
`tui/` (interfaces) · `cli.rs` + `commands.rs` + `main.rs` (CLI). See
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
