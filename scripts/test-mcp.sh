#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mcp_dir="$repo_root/integrations/mcp/agent-handoff-mcp"

cd "$mcp_dir"

if [[ ! -d node_modules ]]; then
  npm ci
fi

node --check server.js
npm ls --omit=dev >/dev/null

echo "MCP smoke test passed."

