# lxp-scan

Cross-repo intelligence CLI for the LeapXpert FE tree:

- **`impact`** — where is a symbol imported and used, with which props?
- **`context`** — LLM-ready context pack for a symbol: definition + usage excerpts
- **`dupes`** — same-name components implemented independently in multiple repos
- **`drift`** — which repos are on diverging versions of `lxp-common-*` packages?
- **`mcp`** — stdio MCP server exposing all of the above to coding agents

AST-based (oxc), tsconfig-alias-aware, parallel. Scans ~4,400 files in ~1s.

## Quick start

```bash
cd ~/Leapxpert/FE
lxp-scan drift
```

> `command not found: lxp-scan` → the terminal predates the install.
> Run `rehash`, or open a new tab. The binary is a symlink at
> `~/.local/bin/lxp-scan` pointing into `target/release`.

```
| package                  | cic-admin-web | lxp-app-admin | lxp-web | lxp-web-client | drift |
| lxp-common-components-js | ^2.1.56       | ^3.1.32       | ^3.1.25 | ^2.0.64        | Major |
```

```bash
lxp-scan impact Button --from lxp-common-components-js
```

```
lxp-web (12 sites)
  src/.../LoginForm.tsx:5
      jsx ×1 · from lxp-common-components-js/components/Button · props: className, disabled
  ...

12 usage site(s) in 12 file(s) across 1 repo(s)
```

## Reading impact output

One header per repo, two lines per usage site:

| Part | Meaning |
|---|---|
| `file:line` | Import statement location, relative to the repo |
| `ref ×N` | Uses as a value/function/type (excludes JSX tags) |
| `jsx ×N` | Times rendered as `<Symbol ...>` |
| `from` | Resolved import source — packages verbatim; intra-repo files prefixed with the repo name, e.g. `lxp-web/src/...` (alias and relative imports of the same file display identically) |
| `props` | Union of props passed across all renders in the file |

## Context packs for LLM agents

```bash
lxp-scan context Avatar --from lxp-common-components-js --root ~/Leapxpert/FE
```

Prints a markdown pack on stdout, ready to paste into a task brief:

- header with totals (sites / files / repos)
- **Definition** — the real declaration behind barrel re-exports, plus its
  `XxxProps` / `IXxxProps` interface when declared in the same file (each
  section capped at 30 lines)
- **Props observed across usages** — per-prop site counts, e.g. `profile ×45`
- **Usage excerpts** — up to `--sites` (default 8) representative sites,
  anchored at the JSX render line (not the import), round-robined across
  repos, preferring unseen prop combinations

Same-name components never blend into one pack: hits are grouped by the repo
defining the imported component, the pack covers the dominant group, and an
"Other components named X" section lists the rest with a ready-to-paste
`--from` hint to repack them. Within the defining repo, when several files
declare the same name the shallowest path wins and a warning names the
alternatives (`--verbose`). `--json` emits the full pack as JSON.

## Recipes

Changing a shared component — what breaks?
```bash
lxp-scan impact Toggle --from lxp-common-components-js --root ~/Leapxpert/FE
```

Changing an intra-repo util (aliases resolved):
```bash
lxp-scan impact formatMessage --from utils/formatMessage --root ~/Leapxpert/FE
```

Common symbol names (Button, Modal): always pass `--from`, or same-named local
symbols across repos will match too.

Machine-readable output:
```bash
lxp-scan impact Button --from lxp-common-components-js --root ~/Leapxpert/FE --json
```

Suspect missing results? Add `--verbose` to see per-file warnings; without it
only a `N warning(s) suppressed` notice is printed.

## Duplicate components

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

## MCP server for Claude Code

```bash
claude mcp add --scope user lxp-scan -- ~/.local/bin/lxp-scan mcp --root ~/Leapxpert/FE
```

Runs a stdio MCP server exposing `impact`, `context`, `drift`, and `dupes` as
tools, so Claude Code pulls cross-repo ground truth itself instead of
guessing. The root is fixed at registration time; tool arguments mirror the
CLI flags (`symbol`, `from`, `sites`).

## Flags

| Flag | Default | Description |
|---|---|---|
| `--root <dir>` | `.` | Directory of repos (repo = subdirectory with a package.json) |
| `--from <substring>` | — | (impact/context) Filter by resolved import source |
| `--sites <n>` | `8` | (context) Maximum usage excerpts |
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
- Hidden directories (e.g. `.claude/worktrees/`) are skipped.
- tsconfig `extends` chains are not followed.

## Troubleshooting

| Problem | Fix |
|---|---|
| `command not found: lxp-scan` | `rehash`, or open a new terminal |
| Stale output after rebuild | Make sure no second copy shadows the symlink: `which -a lxp-scan` must list only `~/.local/bin/lxp-scan` (do NOT `cargo install --path .`) |
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

## Roadmap

- ~~Phase 2: `lxp-scan context <symbol>`~~ — shipped; see "Context packs" above.
