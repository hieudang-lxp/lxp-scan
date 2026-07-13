# lxp-scan

Cross-repo intelligence CLI for the LeapXpert FE tree:

- **`impact`** — where is a symbol imported and used, with which props?
- **`drift`** — which repos are on diverging versions of `lxp-common-*` packages?

AST-based (oxc), tsconfig-alias-aware, parallel. Scans ~4,400 files in ~1s.

## Quick start

```bash
cd ~/Leapxpert/FE
lxp-scan drift
```

> `command not found: lxp-scan` → the terminal predates the install.
> Run `source ~/.cargo/env` once, or open a new tab.

```
| package                  | cic-admin-web | lxp-app-admin | lxp-web | lxp-web-client | drift |
| lxp-common-components-js | ^2.1.56       | ^3.1.32       | ^3.1.25 | ^2.0.64        | Major |
```

```bash
lxp-scan impact Button --from lxp-common-components-js
```

```
| repo    | file:line               | from                                       | refs | jsx | props               |
| lxp-web | src/.../LoginForm.tsx:5 | lxp-common-components-js/components/Button | 0    | 1   | className, disabled |
...
12 usage site(s) in 12 file(s)
```

## Reading the impact table

| Column | Meaning |
|---|---|
| `file:line` | Import statement location, relative to the repo |
| `from` | Resolved import source — packages verbatim; intra-repo files as repo-relative paths (alias and relative imports of the same file display identically) |
| `refs` | Uses as a value/function/type (excludes JSX tags) |
| `jsx` | Times rendered as `<Symbol ...>` |
| `props` | Union of props passed across all renders in the file |

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

## Flags

| Flag | Default | Description |
|---|---|---|
| `--root <dir>` | `.` | Directory of repos (repo = subdirectory with a package.json) |
| `--from <substring>` | — | (impact) Filter by resolved import source |
| `--json` | table | JSON on stdout |
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
| `command not found: lxp-scan` | `source ~/.cargo/env`, or open a new terminal |
| Empty table but usage exists | Check `--from`; try without it; add `--verbose` for parse failures |
| Results differ from grep | See limitations — usually re-exports/namespace imports (skipped) or multi-line imports (grep misses them) |

## Development

```bash
cd ~/tools/lxp-scan
cargo test
cargo clippy --all-targets -- -D warnings
cargo install --path .
cargo run --release -- drift --root ~/Leapxpert/FE
```

## Roadmap

- Phase 2: `lxp-scan context <symbol>` — emit an LLM-ready context pack
  (definition + usage excerpts) for a symbol.
