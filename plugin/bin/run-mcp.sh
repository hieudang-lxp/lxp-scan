#!/bin/bash
# MCP launcher for the lxp-scan plugin. Downloads the release binary on first
# run (cached in ~/.cache/lxp-scan), auto-detects the FE workspace root, and
# execs the stdio server. Claude Code runs this via the plugin's MCP config.
set -euo pipefail

VERSION="v0.3.0"
REPO="hieudang-lxp/lxp-scan"
CACHE_DIR="${HOME}/.cache/lxp-scan"
BIN="${CACHE_DIR}/lxp-scan-${VERSION}"

if [ ! -x "$BIN" ]; then
  mkdir -p "$CACHE_DIR"
  arch="$(uname -m)"
  if [ "$arch" != "arm64" ]; then
    echo "lxp-scan: no prebuilt binary for $(uname -s)/$arch — build from source: https://github.com/${REPO}" >&2
    exit 1
  fi
  curl -fsSL -o "${BIN}.tmp" \
    "https://github.com/${REPO}/releases/download/${VERSION}/lxp-scan-darwin-arm64"
  chmod +x "${BIN}.tmp"
  mv "${BIN}.tmp" "$BIN"
fi

# Convenience: expose the cached binary as `lxp-scan` on PATH so the CLI
# (incl. `lxp-scan tui`) works too. Update only links we created (they point
# into our cache dir); never clobber a dev install or anything else.
LINK="${HOME}/.local/bin/lxp-scan"
link_target="$(readlink "$LINK" 2>/dev/null || true)"
if [ ! -e "$LINK" ] && [ -z "$link_target" ]; then
  mkdir -p "${HOME}/.local/bin"
  ln -s "$BIN" "$LINK"
elif [ -n "$link_target" ] && [ "${link_target#"${CACHE_DIR}"/}" != "$link_target" ]; then
  ln -sf "$BIN" "$LINK"
fi

# Workspace root = the directory whose children are the FE repos.
# Priority: explicit LXP_SCAN_ROOT > parent dir when cwd is itself a repo
# (claude usually runs inside lxp-web etc.) > cwd.
ROOT="${LXP_SCAN_ROOT:-}"
if [ -z "$ROOT" ]; then
  if [ -f "package.json" ]; then
    ROOT="$(dirname "$PWD")"
  else
    ROOT="$PWD"
  fi
fi

exec "$BIN" mcp --root "$ROOT"
