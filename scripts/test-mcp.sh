#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mcp_dir="$repo_root/integrations/mcp/agent-handoff-mcp"

cd "$mcp_dir"

if [[ ! -d node_modules ]]; then
  npm ci
fi

node --check server.js
node --check smoke.mjs
npm ls --omit=dev >/dev/null

cargo build --manifest-path "$repo_root/Cargo.toml" >/dev/null

tmp="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp"
}
trap cleanup EXIT

export HANDOFF_HOME="$tmp/home"
export HANDOFF_PROJECT="$tmp/project"
export HANDOFF_BIN="$repo_root/target/debug/handoff"

mkdir -p "$HANDOFF_PROJECT"
"$HANDOFF_BIN" init >/dev/null
HANDOFF_SESSION_ID=lead-session "$HANDOFF_BIN" session alias lead --project "$HANDOFF_PROJECT" >/dev/null
HANDOFF_SESSION_ID=reviewer-session "$HANDOFF_BIN" session alias reviewer --project "$HANDOFF_PROJECT" >/dev/null
"$HANDOFF_BIN" profile create reviewer --runtime shell --project "$HANDOFF_PROJECT" >/dev/null
export HANDOFF_SESSION_ID=lead-session

node smoke.mjs

echo "MCP smoke test passed."
