#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

cargo build >/dev/null

tmp="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp"
}
trap cleanup EXIT

export HANDOFF_HOME="$tmp/home"
bin="$repo_root/target/debug/handoff"

json_get() {
  python3 -c 'import json,sys; data=json.load(sys.stdin); print(data'"$1"')'
}

wait_for_state() {
  local job_id="$1"
  local want="$2"
  local state=""
  for _ in $(seq 1 30); do
    state="$("$bin" status "$job_id" --json | json_get '["job"]["state"]')"
    if [[ "$state" == "$want" ]]; then
      return 0
    fi
    sleep 0.2
  done
  echo "expected job $job_id to reach $want, got $state" >&2
  "$bin" status "$job_id" --json >&2 || true
  "$bin" logs "$job_id" >&2 || true
  return 1
}

echo "== init/join/actas =="
"$bin" init >/dev/null
"$bin" join demo lead --runtime shell >/dev/null
"$bin" join demo reviewer --runtime shell >/dev/null
"$bin" join demo exclusive --runtime shell >/dev/null
"$bin" actas lead >/dev/null
HANDOFF_SESSION_ID=session-a "$bin" actas exclusive >/dev/null
if HANDOFF_SESSION_ID=session-b "$bin" actas exclusive >/dev/null 2>&1; then
  echo "expected actas lock conflict" >&2
  exit 1
fi
"$bin" actas lead >/dev/null

echo "== send/inbox/history =="
"$bin" to reviewer "Please review" --subject "Review request" >/dev/null
"$bin" post reviewer "Peerpost alias works" >/dev/null
"$bin" actas reviewer >/dev/null
inbox_count="$("$bin" inbox --json | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["messages"]))')"
test "$inbox_count" -eq 2
"$bin" history --json | python3 -c 'import json,sys; assert len(json.load(sys.stdin)["messages"]) >= 2'
"$bin" actas lead >/dev/null
"$bin" to reviewer "Monitor delivery works" >/dev/null
"$bin" monitor --as reviewer --runtime shell --once | grep "Monitor delivery works" >/dev/null
monitor_unread="$("$bin" inbox --as reviewer --json | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["messages"]))')"
test "$monitor_unread" -eq 0

echo "== context =="
"$bin" actas lead >/dev/null
context_id="$("$bin" context create --text "important context" --json | json_get '["context_id"]')"
"$bin" context show "$context_id" --json | python3 -c 'import json,sys; data=json.load(sys.stdin); assert data["context"]["items"][0]["content"] == "important context"'
"$bin" to reviewer --context "$context_id" --message "Use this context" >/dev/null
note="$tmp/note.md"
printf 'file context\n' > "$note"
"$bin" to reviewer --file "$note" --as-context --message "Use this file as context" >/dev/null

echo "== delivery mode hooks =="
"$bin" mode turn --runtime codex --project "$tmp/project" >/dev/null
test -f "$tmp/project/.codex/hooks.json"
grep "handoff" "$tmp/project/.codex/hooks.json" >/dev/null
"$bin" mode off --runtime codex --project "$tmp/project" >/dev/null
test ! -f "$tmp/project/.codex/hooks.json"
"$bin" mode monitor --runtime claude-code --project "$tmp/claude" >/dev/null
test -f "$tmp/claude/.claude/settings.local.json"
grep "SessionStart" "$tmp/claude/.claude/settings.local.json" >/dev/null
grep "monitor --instruction" "$tmp/claude/.claude/settings.local.json" >/dev/null
"$bin" mode both --runtime claude-code --project "$tmp/claude" >/dev/null
grep '"Stop"' "$tmp/claude/.claude/settings.local.json" >/dev/null
"$bin" mode off --runtime claude-code --project "$tmp/claude" >/dev/null
test ! -f "$tmp/claude/.claude/settings.local.json"

echo "== background job =="
job_id="$("$bin" run reviewer --task 'printf "done: %s\n" "$HANDOFF_TASK"' --context "$context_id" --json | json_get '["job_id"]')"
wait_for_state "$job_id" succeeded
"$bin" logs "$job_id" | grep "done:" >/dev/null
"$bin" result "$job_id" | grep "done:" >/dev/null
quick_timeout_id="$("$bin" run reviewer --task 'printf quick' --timeout 5 --as lead --json | json_get '["job_id"]')"
wait_for_state "$quick_timeout_id" succeeded

echo "== retry =="
retry_id="$("$bin" retry "$job_id" --json | json_get '["job_id"]')"
"$bin" status "$retry_id" --json | python3 -c 'import json,sys; data=json.load(sys.stdin); assert data["job"]["retry_of_job_id"]'

echo "== blocked non-shell runtime =="
"$bin" join demo codexbot --runtime codex >/dev/null
blocked_id="$("$bin" run codexbot --task "review this" --as lead --json | json_get '["job_id"]')"
wait_for_state "$blocked_id" blocked

echo "== timeout =="
timeout_id="$("$bin" run reviewer --task 'sleep 2' --timeout 1 --as lead --json | json_get '["job_id"]')"
wait_for_state "$timeout_id" timeout

echo "CLI smoke test passed."
