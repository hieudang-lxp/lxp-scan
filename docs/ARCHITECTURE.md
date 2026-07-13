# Architecture

## Purpose

`lxp-scan` answers two cross-repo questions about the LeapXpert FE workspace
(~9 sibling repos consuming shared `lxp-common-*` packages):

1. **Impact** — before changing a shared symbol: which files import it, how is
   it used (JSX renders, value/type references), and which props are passed?
2. **Drift** — which repos are on diverging versions of the shared packages?

Both must be trustworthy enough to replace manual grepping, which misses
multi-line imports, cannot unify tsconfig-alias vs relative imports, and
cannot see props.

## Pipeline

```
main.rs (clap dispatch, exit codes, stdout=data / stderr=warnings+summary)
  │
  ├─ drift:  discover ──► drift ──► report
  │
  └─ impact: discover ──► per repo: resolver + walker
                            │
                            ▼  rayon par_iter over files
                          prefilter ──► analyzer ──► resolve source
                            │
                            ▼
                          sort (repo, file, line) ──► report
```

| Module | Responsibility |
|---|---|
| `discover` | Find repos = first-level non-hidden dirs with a `package.json`; parse deps (`dependencies` + `devDependencies` merged) |
| `drift` | Group tracked packages (`lxp-common-`, `lxp-design-system`) across repos; flag `Major`/`Minor`/`Same` by highest diverging semver component (patch ignored; unparseable versions skipped) |
| `walker` | Enumerate `.ts/.tsx/.js/.jsx` via the `ignore` crate — skips `node_modules`/`build`/`dist`/`coverage`, hidden dirs, `.gitignore` matches, and `.d.ts` |
| `resolver` | Per-repo tsconfig (JSONC-tolerant): `baseUrl` + `paths` aliases. Canonicalizes alias and relative specifiers to the same absolute file path (lexical normalize + extension probing), so both forms compare and filter identically. Exact (star-less) patterns match on full equality only; longest prefix wins |
| `analyzer` | One-file oxc AST pass: find imports of the symbol (named imports match the *source* name incl. `as` renames; default imports match the local name), then count identifier refs, JSX uses, and collect JSX prop names for the local binding |
| `impact` | Orchestration: parallel scan, `--from` filter on the *resolved* source, warning aggregation, deterministic sort |
| `report` | Table (TTY: minimal borders, dynamic width, colored drift levels) and JSON rendering |

## Key decisions

- **Prefilter before parse.** `text.contains(symbol)` skips ~90% of files for
  near-zero cost before any AST work. Sound because every match form requires
  the symbol string to appear literally in the import statement.
- **AST, not regex.** Symbol names collide constantly (`Button` as a local
  const, `ButtonGroup` as a substring). oxc gives exact import/JSX semantics.
- **Never abort a scan.** Every per-item failure (unreadable file, parse
  error, broken tsconfig/package.json) becomes a warning and the scan
  continues. Warnings print under `--verbose`; otherwise a one-line
  suppressed-count notice. Only an unreadable `--root` is a hard error.
- **JSONC everywhere config is read.** tsconfig.json contains comments and
  trailing commas in the real repos; `jsonc-parser` handles both.
- **`.js` parses with JSX enabled.** lxp-web has JSX in plain `.js` files;
  oxc's default `SourceType` for `.js` would reject 36 real files. `.ts`
  stays JSX-off (ambiguous with generics).
- **Deterministic output.** Results sorted after the parallel collect; two
  runs produce byte-identical JSON.
- **TTY-aware rendering.** Colors and width-wrapping only when stdout is a
  terminal; piped output stays plain and one-line-per-row.

## oxc 0.139 gotchas (hard-won, do not rediscover)

- The `Visit` trait lives in the separate `oxc_ast_visit` crate.
- A capitalized JSX tag (`<Button>`) is a real `IdentifierReference`; the
  default walk visits it for both opening and closing tags. The analyzer
  overrides `visit_jsx_element_name` as a no-op and counts member-expression
  tag roots (`<Button.Icon>`) explicitly in `visit_jsx_opening_element`.
- `ParserReturn` exposes `diagnostics` (not `errors`); parse failure is
  `panicked || (has_errors && empty body)` — error-tolerant files with a
  usable AST are still scanned.

## Semantics (what the numbers mean)

- `refs` = identifier references to the imported binding: value, function,
  and **type-position** uses (type usage is impact), plus roots of
  member-expression JSX tags. Excludes the import itself and JSX tag names.
- `jsx` = renders as a plain `<Symbol …>` element; `props` = union of
  attribute names across those renders.
- Not followed (v1): namespace imports, re-export chains, dynamic `import()`,
  `require()`. No scope analysis — same-name shadowing counts.

## Testing

- Unit tests per module (`#[cfg(test)]`), including negative cases: substring
  collisions, local shadowing, malformed JSON/JSONC, exact-pattern aliases,
  hidden dirs.
- Integration tests (`tests/impact_it.rs`) run the full pipeline over two
  fixture mini-repos in `tests/fixtures/workspace/` — one with tsconfig
  aliases and deliberate JSONC quirks, one without tsconfig, plus broken
  files/repos for the warning paths. The fixture `node_modules` dir is
  committed intentionally (the walker test needs it).
- Real-world validation: results cross-checked against independent grep over
  `~/Leapxpert/FE` (every discrepancy explained before acceptance).

## Roadmap

- Phase 2: `context <symbol>` — emit an LLM-ready context pack (definition +
  usage excerpts) built on the `impact` machinery.
