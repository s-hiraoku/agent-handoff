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

echo "== init/session/profile =="
"$bin" --version | grep -Fx "handoff 0.3.0" >/dev/null
"$bin" init >/dev/null
HANDOFF_SESSION_ID=lead-session "$bin" session alias lead >/dev/null
HANDOFF_SESSION_ID=reviewer-session "$bin" session alias reviewer >/dev/null
"$bin" profile create reviewer --runtime shell >/dev/null

echo "== send/inbox/history =="
HANDOFF_SESSION_ID=lead-session "$bin" to reviewer "Please review" --subject "Review request" >/dev/null
HANDOFF_SESSION_ID=lead-session "$bin" post reviewer "Peerpost alias works" >/dev/null
inbox_count="$(HANDOFF_SESSION_ID=reviewer-session "$bin" inbox --json | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["messages"]))')"
test "$inbox_count" -eq 2
HANDOFF_SESSION_ID=lead-session "$bin" history --json | python3 -c 'import json,sys; assert len(json.load(sys.stdin)["messages"]) >= 2'
HANDOFF_SESSION_ID=lead-session "$bin" to reviewer "Peek safety" >/dev/null
peek_count="$(HANDOFF_SESSION_ID=reviewer-session "$bin" inbox --peek --json | python3 -c 'import json,sys; data=json.load(sys.stdin); assert data["marked_read"] is False; print(len(data["messages"]))')"
test "$peek_count" -eq 1
after_peek_count="$(HANDOFF_SESSION_ID=reviewer-session "$bin" inbox --peek --json | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["messages"]))')"
test "$after_peek_count" -eq 1
HANDOFF_SESSION_ID=reviewer-session "$bin" inbox >/dev/null
cleared_count="$(HANDOFF_SESSION_ID=reviewer-session "$bin" inbox --peek --json | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["messages"]))')"
test "$cleared_count" -eq 0
HANDOFF_SESSION_ID=lead-session "$bin" to reviewer "Monitor delivery works" >/dev/null
HANDOFF_SESSION_ID=reviewer-session "$bin" monitor --runtime shell --once | grep "Monitor delivery works" >/dev/null
monitor_unread="$(HANDOFF_SESSION_ID=reviewer-session "$bin" inbox --json | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["messages"]))')"
test "$monitor_unread" -eq 0
HANDOFF_SESSION_ID=lead-session "$bin" to reviewer "Notify delivery works" >/dev/null
"$bin" daemon --once >/dev/null
HANDOFF_SESSION_ID=reviewer-session "$bin" notify | grep "Notify delivery works" >/dev/null

echo "== context =="
context_id="$(HANDOFF_SESSION_ID=lead-session "$bin" context create --text "important context" --json | json_get '["context_id"]')"
"$bin" context show "$context_id" --json | python3 -c 'import json,sys; data=json.load(sys.stdin); assert data["context"]["items"][0]["content"] == "important context"'
HANDOFF_SESSION_ID=lead-session "$bin" to reviewer --context "$context_id" --message "Use this context" >/dev/null
note="$tmp/note.md"
printf 'file context\n' > "$note"
HANDOFF_SESSION_ID=lead-session "$bin" to reviewer --file "$note" --as-context --message "Use this file as context" >/dev/null

echo "== delivery mode hooks =="
"$bin" mode turn --runtime codex --project "$tmp/project" >/dev/null
test -f "$tmp/project/.codex/hooks.json"
grep "handoff" "$tmp/project/.codex/hooks.json" >/dev/null
grep -- "--project" "$tmp/project/.codex/hooks.json" >/dev/null
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
job_id="$(HANDOFF_SESSION_ID=lead-session "$bin" run reviewer --task 'printf "done: %s\n" "$HANDOFF_TASK"' --context "$context_id" --json | json_get '["job_id"]')"
wait_for_state "$job_id" succeeded
"$bin" status "$job_id" | grep "state: succeeded" >/dev/null
"$bin" logs "$job_id" | grep "done:" >/dev/null
"$bin" result "$job_id" | grep "done:" >/dev/null
quick_timeout_id="$(HANDOFF_SESSION_ID=lead-session "$bin" run reviewer --task 'printf quick' --timeout 5 --json | json_get '["job_id"]')"
wait_for_state "$quick_timeout_id" succeeded

echo "== delegate =="
# shellcheck disable=SC2016
HANDOFF_SESSION_ID=lead-session "$bin" delegate reviewer --task 'printf "delegated: %s\n" "$HANDOFF_TASK"' --wait | grep "delegated:" >/dev/null
# shellcheck disable=SC2016
printf 'stdin delegate context\n' | HANDOFF_SESSION_ID=lead-session "$bin" delegate reviewer --task 'printf "%s\n" "$HANDOFF_CONTEXT"' --stdin --wait | grep "stdin delegate context" >/dev/null

echo "== retry =="
retry_id="$("$bin" retry "$job_id" --json | json_get '["job_id"]')"
"$bin" status "$retry_id" --json | python3 -c 'import json,sys; data=json.load(sys.stdin); assert data["job"]["retry_of_job_id"]'

echo "== blocked non-shell runtime =="
"$bin" profile create geminibot --runtime gemini >/dev/null
blocked_id="$(HANDOFF_SESSION_ID=lead-session "$bin" run geminibot --task "review this" --json | json_get '["job_id"]')"
wait_for_state "$blocked_id" blocked
"$bin" status "$blocked_id" | grep "HANDOFF_RUNTIME_CMD_GEMINI" >/dev/null

echo "== timeout =="
timeout_id="$(HANDOFF_SESSION_ID=lead-session "$bin" run reviewer --task 'sleep 2' --timeout 1 --json | json_get '["job_id"]')"
wait_for_state "$timeout_id" timeout

echo "== cancel state is terminal =="
cancel_id="$(HANDOFF_SESSION_ID=lead-session "$bin" run reviewer --task 'sleep 2; printf should-not-finish' --json | json_get '["job_id"]')"
sleep 0.2
"$bin" cancel "$cancel_id" >/dev/null
wait_for_state "$cancel_id" cancelled
sleep 0.5
cancel_state="$("$bin" status "$cancel_id" --json | json_get '["job"]["state"]')"
test "$cancel_state" = cancelled

echo "== follow logs =="
follow_id="$(HANDOFF_SESSION_ID=lead-session "$bin" run reviewer --task 'printf "follow-one\n"; sleep 0.3; printf "follow-two\n"' --json | json_get '["job_id"]')"
"$bin" logs "$follow_id" --follow | grep "follow-two" >/dev/null
wait_for_state "$follow_id" succeeded

echo "CLI smoke test passed."
