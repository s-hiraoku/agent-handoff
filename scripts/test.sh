#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

echo "== cargo fmt --check =="
cargo fmt --check

echo "== cargo check =="
cargo check

echo "== cargo test =="
cargo test

echo "== CLI smoke test =="
./scripts/smoke.sh

echo "== MCP smoke test =="
./scripts/test-mcp.sh

echo "== release build =="
cargo build --release

echo "All tests passed."

