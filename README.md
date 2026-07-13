# lxp-scan

Cross-repo FE intelligence CLI for the LeapXpert frontend tree. Point it at a
directory of repos (e.g. `~/Leapxpert/FE`) and it answers two questions:

- **impact** — where is a symbol imported and used across all repos?
- **drift** — which repos are on diverging versions of the shared
  `lxp-common-*` / `lxp-design-system` packages?

Scans are AST-based (oxc), tsconfig-alias-aware, parallel per file, and never
abort on a broken file — problems become warnings.

## Quick start

The binary is already installed at `~/.cargo/bin/lxp-scan` and `~/.zshenv`
puts it on PATH — **open a new terminal** (or run `source ~/.cargo/env` in an
old one) and:

```bash
# Version drift of shared lxp-common-* packages across all repos
lxp-scan drift --root ~/Leapxpert/FE

# Who uses Button from the shared component lib, with which props
lxp-scan impact Button --from lxp-common-components-js --root ~/Leapxpert/FE

# An intra-repo symbol, resolved through tsconfig aliases
lxp-scan impact formatMessage --from utils/formatMessage --root ~/Leapxpert/FE

# JSON output (for scripts / AI context)
lxp-scan impact Toggle --root ~/Leapxpert/FE --json
```

Tip: `cd ~/Leapxpert/FE` first and you can drop `--root` entirely
(it defaults to the current directory): `lxp-scan drift`.

## Install / update

From the repo root (`~/tools/lxp-scan`):

```
cargo install --path .
```

Installs (or replaces) `lxp-scan` in `~/.cargo/bin`. To try changes without
installing, run from source: `cargo run --release -- drift --root ~/Leapxpert/FE`.

## Usage

### impact

Find usage sites of a symbol across every repo under `--root`:

```
lxp-scan impact Button --from lxp-common-components-js --root ~/Leapxpert/FE
```

```
+----------------+---------------------------------------------------+---------------------------------------------+------+-----+----------------------------+
| repo           | file:line                                         | from                                        | refs | jsx | props                      |
+============================================================================================================================================================+
| lxp-web        | src/modules/Authenticate/components/LoginForm.tsx:5 | lxp-common-components-js/components/Button | 0    | 1   | className, disabled, ...   |
...
12 usage site(s) in 12 file(s)
```

Each row is one import site: the resolved import source, non-JSX identifier
references (`refs`), JSX renders of the component (`jsx`), and the union of
JSX props used. Intra-repo imports are resolved through tsconfig
`baseUrl`/`paths`, so alias and relative imports of the same file display as
the same repo-relative path and match the same `--from` filter:

```
lxp-scan impact formatMessage --from utils/formatMessage --root ~/Leapxpert/FE
```

Flags:

- `--from <substring>` — keep only hits whose resolved import source contains
  the substring (package name verbatim, repo-relative path for local files)
- `--root <dir>` — workspace root containing the repos (default `.`)
- `--json` — machine-readable output instead of the table
- `--verbose` — print per-file warnings (parse failures, malformed
  package.json/tsconfig) to stderr; without it a one-line
  `N warning(s) suppressed` notice is shown

### drift

Compare `lxp-common-*` / `lxp-design-system` dependency versions across repos:

```
lxp-scan drift --root ~/Leapxpert/FE
```

```
| package                  | cic-admin-web | lxp-app-admin | lxp-web | lxp-web-client | ... | drift |
| lxp-common-components-js | ^2.1.56       | ^3.1.32       | ^3.1.25 | ^2.0.64        |     | Major |
```

Rows are marked `Major`, `Minor`, or `Same` by the highest semver component
that differs across repos. Same flags: `--root`, `--json`, `--verbose`.

## Known v1 limitations

- Namespace imports (`import * as X`), re-export chains
  (`export { X } from ...`), dynamic `import()`, and `require()` are not
  followed.
- Type-position references count toward impact (a `TButtonProps` usage of an
  imported type is a ref like any other).
- Member JSX tags (`<Button.Icon />`) are reported as refs to the root
  identifier, not as `jsx` uses.
- Drift ignores patch-level differences and skips version strings it cannot
  parse (`workspace:*`, `latest`, git URLs).
- A "repo" is a first-level directory under `--root` containing a
  `package.json`.
- Hidden directories (dotfiles, e.g. `.claude/worktrees/`) are skipped, in
  addition to `node_modules`/`build`/`dist`/`coverage` and `.gitignore` rules —
  a grep may therefore "find" hits the scanner correctly excludes.
- tsconfig `extends` chains are not followed; a repo whose `paths`/`baseUrl`
  live in an extended base config silently loses alias resolution (no FE repo
  does this today).

## Development

```
cargo test
cargo clippy --all-targets -- -D warnings
cargo install --path .
```

Integration tests run against the mini-workspace in `tests/fixtures/`.

## Roadmap

- Phase 2: `context` command — emit an LLM-ready context pack (definition +
  usage excerpts) for a symbol.
