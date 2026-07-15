---
name: lxp-scan
description: Use when working in the LeapXpert FE repos and you need cross-repo ground truth - before changing a shared component/util (blast radius), when writing code that uses an unfamiliar component (real props/definition), or when assessing duplicate implementations or package version drift.
---

# lxp-scan — cross-repo FE ground truth

The `lxp-scan` MCP server exposes five tools that scan all LeapXpert FE repos
with AST accuracy (tsconfig-alias-aware, catches renamed and multi-line
imports that grep misses). Prefer these over grep for cross-repo questions.

## Which tool when

| Situation | Tool | Arguments |
|---|---|---|
| About to change a shared symbol — who breaks? | `impact` | `symbol`, optional `from` |
| Using/refactoring a component you haven't read | `context` | `symbol`, optional `from`, `sites` |
| Suspect the same component exists in several repos | `dupes` | — |
| Suspect a util is reimplemented under different names | `clones` | optional `symbol`, `min_tokens`, `same_file` |
| Version questions about `lxp-common-*` packages | `drift` | — |

## Rules

- **Always pass `from` for common names** (Button, Modal, Avatar,
  ConfirmPopup…). Several repos implement same-name components; without
  `from` you get the dominant one and a "NOT in this pack" section listing
  the others — repack with the suggested `--from` value if you meant that one.
- `impact` counts type-position references as impact; `<Button.Icon />`
  counts as a ref of `Button`, not a JSX render.
- Not followed (v1): namespace imports (`import * as X`), re-export chains,
  dynamic `import()`. Zero hits does not prove zero usage for those patterns.
- The `context` pack's "Props observed across usages" is measured from real
  call sites — trust it over your recollection of the component's API.
- `clones` finds EXACT structural clones (identical bodies after
  normalizing identifiers/whitespace; literals kept). A reimplementation
  with extra guards or a different algorithm will NOT appear — absence of a
  cluster is not proof a util isn't duplicated in spirit. Its notes
  cross-check names against npm-only `lxp-common-*` exports; the footer
  lists packages the scan cannot see into.
- Cite `repo/file:line` from tool output when reporting findings.
