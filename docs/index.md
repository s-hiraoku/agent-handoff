# agent-handoff User Guide

`agent-handoff` is a local-first app for coding agents that need to pass messages, context, and work to each other.

It is built around one command:

```sh
handoff
```

## Concepts

Team:
: A local group of agents that can message each other.

Agent:
: A named identity such as `lead`, `reviewer`, `coder`, or `researcher`.

Active role:
: The agent identity used by commands in the current project.

Thread:
: A conversation or task handoff history.

Context:
: Captured information such as text, files, git diff, stdin, or command output.

Job:
: A background task started for another agent.

## Install

```sh
git clone https://github.com/s-hiraoku/agent-handoff.git
cd agent-handoff
cargo install --path .
handoff init
```

## First Handoff

```sh
handoff join demo lead --runtime shell
handoff join demo reviewer --runtime shell
handoff actas lead
handoff to reviewer "Please review this change."
handoff actas reviewer
handoff inbox
```

`handoff actas <agent>` selects the current sending and receiving role. When the host exposes a stable session id, handoff claims a lease for that role so peer sessions do not stream the same role's inbox.

Use `handoff inbox --peek` to inspect unread messages without marking them read. This is useful when an agent or script wants to decide whether to act before consuming the inbox.

## Send Like peerpost

Short message:

```sh
handoff to reviewer "Check parser.rs"
```

Compatibility alias:

```sh
handoff post reviewer "Check parser.rs"
```

Pipe command output:

```sh
cargo test 2>&1 | handoff to reviewer --stdin --subject "Test output"
```

Send a note file:

```sh
handoff to reviewer --file notes.md
```

Reply to a thread:

```sh
handoff reply <thread-id> "Additional constraint: do not change the public API."
```

## Use Context

Create a context package:

```sh
handoff context create --git-diff
handoff context create --file notes.md
handoff context create --cmd "cargo test"
```

Show context:

```sh
handoff context show <context-id>
```

Send context:

```sh
handoff to reviewer --context <context-id> --message "Use this context."
handoff to reviewer --git-diff --message "Review this diff."
handoff to reviewer --file notes.md --as-context --message "Use these notes."
```

## Run Background Work

For `shell` runtime, the task text is executed as a shell command:

```sh
handoff run reviewer --task "echo reviewed"
```

With context:

```sh
handoff run reviewer --task 'echo "$HANDOFF_CONTEXT" | wc -l' --context <context-id>
handoff run reviewer --task 'long-running-command' --timeout 30
```

Track it:

```sh
handoff status <job-id>
handoff logs <job-id>
handoff result <job-id>
```

`handoff status <job-id>` prints a compact summary for humans. Use `handoff status <job-id> --json` for scripts and agent integrations. Blocked, failed, and timed-out jobs include the next command to check logs or retry.

Cancel or retry:

```sh
handoff cancel <job-id>
handoff retry <job-id>
```

## Runtime Adapters

For non-shell runtimes, set an adapter command. Per-agent adapter:

```sh
export HANDOFF_AGENT_CMD_REVIEWER='reviewer-agent --task "$HANDOFF_TASK"'
```

Per-runtime adapter:

```sh
export HANDOFF_RUNTIME_CMD_CODEX='codex exec "$HANDOFF_TASK"'
```

The adapter receives:

```text
HANDOFF_JOB_ID
HANDOFF_TASK
HANDOFF_CONTEXT
```

## Delivery Modes

Configure project-local delivery hooks:

```sh
handoff mode turn --runtime codex
handoff mode monitor --runtime claude-code
handoff mode both --runtime claude-code
handoff mode off --runtime codex
```

`turn` writes a Stop/inbox hook for supported runtimes. `monitor` writes a Claude Code SessionStart hook that asks the host to launch a persistent inbox stream. `both` installs both paths. `off` removes handoff-owned hook entries and leaves unrelated hooks in place.

Run the stream directly when the host does not manage it:

```sh
handoff monitor --as reviewer --runtime shell
handoff monitor --as reviewer --runtime shell --once
```

## JSON for Agents

Use `--json` when another tool or agent is calling `handoff`:

```sh
handoff inbox --json
handoff status <job-id> --json
handoff context show <context-id> --json
```

## MCP

The MCP server lives at:

```text
integrations/mcp/agent-handoff-mcp/
```

It exposes tools for send, inbox, context creation, run, status, logs, and result.

Set `HANDOFF_PROJECT=/absolute/path/to/project` in the MCP config when the MCP host may start outside your repository. Each MCP tool also accepts an optional `project` argument; the server uses that value first, then `HANDOFF_PROJECT`, then its own current working directory.

The MCP server exposes tools for sending, replying, inbox reads, history, message/context lookup, context creation from text/git diff/commands/files, jobs, logs, results, cancel, and retry.

## Testing

Run the full local suite:

```sh
./scripts/test.sh
```

Focused checks:

```sh
make fmt
make check
make smoke
make mcp
make release-test
```

The full suite covers Rust formatting, compile checks, unit tests, CLI smoke tests, MCP checks, and release build.

## Privacy

MVP behavior:

- Local storage only.
- No accounts.
- No telemetry.
- No required network service.
- Messages, context, job logs, and results are stored under `HANDOFF_HOME`.

## Troubleshooting

`not_joined`
: Run `handoff join <team> <agent>` in the project.

`multiple_identities`
: Run `handoff actas <agent>` or pass `--as <agent>`.

`blocked`
: The target runtime has no adapter command. Set `HANDOFF_AGENT_CMD_<AGENT>` or `HANDOFF_RUNTIME_CMD_<RUNTIME>`.

`job_not_finished`
: The job has not produced a result yet. Check `handoff status <job-id>` and `handoff logs <job-id>`.
