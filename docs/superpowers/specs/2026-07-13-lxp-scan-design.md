# lxp-scan — Cross-repo FE intelligence CLI (design)

Date: 2026-07-13
Status: approved by Hieu

## Purpose

A personal Rust CLI that answers cross-repo questions about the frontend repos
under `~/Leapxpert/FE` that today require manual grepping:

1. **Impact analysis** — "which files, across all FE apps, use symbol X, and
   with which props?" Needed whenever a shared component or util changes.
2. **Version drift** — "which version of each `lxp-common-*` package is each
   repo on?" Real drift exists today (`lxp-common-components-js` spans
   `2.0.64` → `3.1.32` across the four apps).

Non-goals for v1: the `context` command (agent-ready markdown context packs)
is deferred to phase 2. No backend interaction of any kind; the tool only
reads local source trees.

## Verified facts the design rests on

- Four consumer apps: `lxp-web` (~1781 src files), `lxp-app-admin` (~2008),
  `cic-admin-web` (~211), `lxp-web-client` (~194).
- 946 files across the four apps import from `lxp-common-*` packages.
- Shared-lib imports use subpaths, e.g.
  `import { Button } from 'lxp-common-components-js/components/Button'`.
- Apps use tsconfig path aliases for intra-repo imports, e.g.
  `import { formatMessage } from 'utils/formatMessage'` in `lxp-app-admin`.

## CLI surface

```
lxp-scan impact <symbol> [--from <module>] [--root <dir>] [--json] [--verbose]
lxp-scan drift [--root <dir>] [--json] [--verbose]
```

- `impact <symbol>`: scan every repo under `--root`, list files importing
  `<symbol>` with `repo`, `file:line`, import source, reference count, and
  JSX prop names when the symbol is used as a JSX element.
- `--from <module>`: substring filter on the *resolved* import source. Matches
  both package specifiers (`lxp-common-components-js`) and canonicalized
  intra-repo module paths (`utils/commons`, which also catches equivalent
  relative imports).
- `drift`: table of `lxp-common-*` and `lxp-design-system` dependency versions
  per repo, flagging major/minor divergence.
- `--root` defaults to the current directory. A "repo" is any first-level
  subdirectory of root containing a `package.json`.
- Default output: human-readable table. `--json`: machine-readable.

## Architecture

Single binary crate. Modules:

| Module | Responsibility |
|---|---|
| `main.rs` | CLI definition (`clap`), command dispatch |
| `discover.rs` | Find repos under root; parse each `package.json` (`serde_json`); feeds `drift` |
| `walker.rs` | Enumerate `.ts/.tsx/.js/.jsx` files via the `ignore` crate (respects `.gitignore`; skips `node_modules`, `build`, `dist`, `coverage`) |
| `resolver.rs` | Parse each repo's `tsconfig.json` (`baseUrl`, `paths`); classify import specifiers as package / relative / alias; canonicalize alias and relative specifiers to absolute file paths so both forms compare equal |
| `analyzer.rs` | Per file: cheap substring prefilter for the symbol name, then `oxc_parser` AST parse; extract imports of the symbol, identifier reference counts, and JSX attribute names |
| `report.rs` | Table and JSON rendering |

Scanning strategy (approach C, chosen over full-AST and regex-only):
read every candidate file (necessary anyway), run a substring check for the
symbol name, and AST-parse only files that hit. Parallelized with `rayon`
over the file list. This gives regex-level speed with AST-level accuracy —
accuracy is essential because arbitrary symbol names collide often.

## Impact semantics (v1)

- A file matches when it **imports** the symbol via a named import
  (`import { X } from ...`, including `as` renames — match on the source name)
  or a default import (`import X from ...` — match on the local name).
- Within a matching file, report: total identifier references to the imported
  binding, and, when used as a JSX element, the set of attribute (prop) names.
- Known v1 limitations (explicitly out of scope): re-export chains
  (`export * from`, barrel forwarding), namespace imports
  (`import * as X`), dynamic `import()`, `require()`.

## Error handling

- A file that fails to parse is skipped with a warning under `--verbose`;
  the scan never aborts because of one bad file.
- A repo with a missing or malformed `tsconfig.json` loses only alias
  resolution (package and relative imports still work); a warning is emitted.
- Malformed `package.json` excludes that repo from `drift` with a warning.
- Nonexistent `--root`: clear error, exit code 1.
- Exit codes: 0 on success (including zero matches), 1 on usage/IO errors.
- tsconfig/package.json files may contain comments or trailing commas
  (JSONC); the parser used for them must tolerate this (e.g. `jsonc-parser`
  style handling or a lenient parse), falling back to a warning if unreadable.

## Testing

- Unit tests: `resolver` (alias mapping: `baseUrl`-only, `paths` with
  wildcards, relative canonicalization), `analyzer` (fixture source files →
  expected imports/refs/props).
- Integration: two fake mini-repos under `tests/fixtures/` (one with tsconfig
  aliases, one consuming a fake shared package); run the full scan and assert
  the `--json` output.
- Real-world smoke check (manual, not CI): `impact Button --from
  lxp-common-components-js` over `~/Leapxpert/FE` should be consistent with
  the independently measured ~27 importing files.
  *(Post-implementation note: the ~27 figure counted files importing anything
  from the Button module path — mostly `EButtonType`/`ButtonMenu`. Actual
  `Button`-symbol importers: 12, grep-cross-checked; the tool also catches a
  multi-line import grep misses and excludes hidden-worktree duplicates.)*

## Dependencies (Rust crates)

`clap` (CLI), `oxc_parser` + `oxc_ast` + `oxc_span` (TS/JSX parsing),
`ignore` (file walking), `rayon` (parallelism), `serde`/`serde_json`
(JSON in/out), `anyhow` (error plumbing), a table-rendering crate
(`comfy-table`) or manual formatting — implementer's choice.

## Phase 2 (recorded, not designed)

`lxp-scan context <symbol> --format md|json`: emit an agent-ready context
pack (usage sites + surrounding snippets) for feeding AI workflows. Builds
directly on the `impact` machinery.
