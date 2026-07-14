#!/bin/bash
# MCP launcher for the lxp-scan plugin. Downloads the release binary on first
# run (cached in ~/.cache/lxp-scan), auto-detects the FE workspace root, and
# execs the stdio server. Claude Code runs this via the plugin's MCP config.
set -euo pipefail

VERSION="v0.1.0"
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
