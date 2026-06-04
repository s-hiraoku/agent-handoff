#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
worktree="$(mktemp -d)"

cleanup() {
  if git -C "$repo_root" worktree list --porcelain | grep -Fq "worktree $worktree"; then
    git -C "$repo_root" worktree remove "$worktree" --force >/dev/null 2>&1 || true
  else
    rm -rf "$worktree"
  fi
}
trap cleanup EXIT

git -C "$repo_root" fetch origin gh-pages >/dev/null 2>&1 || true

if git -C "$repo_root" show-ref --verify --quiet refs/remotes/origin/gh-pages; then
  git -C "$repo_root" worktree add "$worktree" origin/gh-pages >/dev/null
  git -C "$worktree" switch -C gh-pages >/dev/null
else
  git -C "$repo_root" worktree add -b gh-pages "$worktree" >/dev/null
fi

find "$worktree" -mindepth 1 -maxdepth 1 ! -name .git -exec rm -rf {} +
cp -R "$repo_root/docs/." "$worktree/"
rm -f "$worktree/.nojekyll"

git -C "$worktree" add .
if git -C "$worktree" diff --cached --quiet; then
  echo "Pages are already up to date."
  exit 0
fi

git -C "$worktree" commit -m "Publish user guide to GitHub Pages"
git -C "$worktree" push origin gh-pages

