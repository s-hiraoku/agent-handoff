# agent-handoff

`agent-handoff` is a local-first coordination tool for coding agents.

It combines three capabilities in one CLI:

- `agmsg`-style local agent messaging: teams, identities, inbox, history, delivery modes.
- `peerpost`-style low-friction sending: `handoff to`, `handoff post`, stdin, files, replies.
- `rig`-style task handoff: context packages, background jobs, status, logs, results, cancel, retry.

The command is:

```sh
handoff
```

## Status

MVP implementation. The storage format and CLI may still change before `v1.0`, but the current release is usable for local agent-to-agent messaging, context passing, and profile-backed background tasks.

## Install

From source:

```sh
git clone https://github.com/s-hiraoku/agent-handoff.git
cd agent-handoff
cargo install --path .
```

Check:

```sh
handoff init
handoff --help
```

By default, local state is stored in:

```text
~/.handoff/
```

For tests or isolated projects:

```sh
export HANDOFF_HOME=/tmp/my-handoff
```

## Quick Start

Create a live session alias and a delegation profile:

```sh
handoff init
handoff session alias lead
HANDOFF_SESSION_ID=reviewer-session handoff session alias reviewer
handoff profile create reviewer --runtime shell
handoff profile set reviewer session=@reviewer capability=review
```

Send a message:

```sh
handoff to @reviewer "Please review the current diff."
```

Read as the recipient:

```sh
HANDOFF_SESSION_ID=reviewer-session handoff inbox
```

Preview without marking messages read:

```sh
handoff inbox --peek
```

Send command output or a file:

```sh
git diff | handoff to @reviewer --stdin --subject "Current diff"
handoff to @reviewer --file notes.md
```

Create and send context:

```sh
CTX=$(handoff context create --git-diff --json | jq -r .context_id)
handoff to @reviewer --context "$CTX" --message "Use this diff as context."
handoff to @reviewer --git-diff --message "Review this diff."
handoff to @reviewer --file notes.md --as-context --message "Use these notes."
handoff context show "$CTX"
```

Run a background task:

```sh
handoff run reviewer --task 'echo reviewed: "$HANDOFF_TASK"'
handoff status
handoff logs <job-id>
handoff logs <job-id> --follow
handoff result <job-id>
```

Delegate and wait for a result in one command:

```sh
git diff | handoff delegate reviewer --stdin --task "Review this diff" --wait
```

`handoff status <job-id>` prints a short human-readable summary by default. `handoff logs --follow` streams new adapter/stdout/stderr log lines until the job reaches a terminal state. Use `--json` when scripting.

For `shell` runtime, the task text is executed by the shell. `claude-code`, `codex`, and `copilot` have built-in adapters that call `claude -p "$HANDOFF_PROMPT" --output-format json`, `codex exec "$HANDOFF_PROMPT" --json`, and `copilot -p "$HANDOFF_PROMPT" --output-format=json --allow-all-tools`. For other runtimes, or to override any built-in adapter, set an adapter command:

```sh
export HANDOFF_AGENT_CMD_REVIEWER='my-reviewer-agent --task "$HANDOFF_TASK"'
handoff run reviewer --task "Review the diff" --context "$CTX"
```

The spawned process receives:

```text
HANDOFF_JOB_ID
HANDOFF_TASK
HANDOFF_CONTEXT
HANDOFF_PROMPT
```

## Core Commands

Profiles and sessions:

```sh
handoff setup claude-code
handoff profile create <profile> --runtime shell
handoff profile create <profile> --runtime codex --prompt-file reviewer.md
handoff profile create <profile> --runtime copilot
handoff profile list
handoff profile set <profile> model=...
handoff profile set reviewer session=@reviewer capability=review
handoff session alias <alias>
handoff sessions
handoff whoami
handoff active
```

The current sender is inferred from `HANDOFF_SESSION_ID`, `CLAUDE_CODE_SESSION_ID`, `CODEX_SESSION_ID`, `COPILOT_SESSION_ID`, `GITHUB_COPILOT_SESSION_ID`, or a project-local fallback session id. Use `handoff session alias <alias>` to give the current live session a readable `@alias` address. Profiles keep bare names for background execution; if a session and profile share a name, use `handoff to @alias` for inbox delivery and `handoff delegate <profile>` for worker execution.

Messaging:

```sh
handoff send @<alias> <message>
handoff to @<alias> <message>
handoff to @<alias> --project /path/to/repo <message>
handoff post @<alias> <message>
handoff route --capability review <message>
handoff reply <thread-id> <message>
handoff inbox
handoff notify
handoff notify --hook --json
handoff history
handoff show <message-id>
```

Context:

```sh
handoff context create --text "notes"
handoff context create --stdin
handoff context create --file notes.md
handoff context create --files notes.md --files plan.md
handoff context create --git-diff
handoff context create --cmd "cargo test"
handoff context show <context-id>
handoff context list
```

Jobs:

```sh
handoff run <profile> --task <text>
handoff run <profile> --task <text> --timeout 30
handoff delegate <profile> --task <text> --wait
handoff delegate <profile> --stdin --task <text> --wait
handoff status [job-id]
handoff logs <job-id>
handoff logs <job-id> --follow
handoff result <job-id>
handoff cancel <job-id>
handoff retry <job-id>
```

Delivery mode:

```sh
handoff mode
handoff mode turn
handoff mode turn --runtime copilot
handoff mode monitor --runtime claude-code
handoff mode both --runtime claude-code
handoff mode off
handoff daemon
handoff monitor --runtime shell
handoff reset
handoff install-alias agent-handoff
```

`mode turn` writes a project-local notification hook for supported runtimes, including `.codex/hooks.json` for Codex and `.github/hooks/handoff.json` for Copilot. The hook calls `handoff notify --hook --json`, which emits a host-agent response that asks the receiving agent to process new messages immediately. `handoff daemon` writes `.handoff/notify/<session>.md` files for live sessions, and `handoff notify` consumes the current session notification file. `mode monitor` writes a Claude Code `SessionStart` hook that asks the host to launch a persistent `handoff monitor` stream. `mode both` installs both delivery paths. `mode off` removes handoff-owned hook entries.

Project maintenance commands such as `reset` and `install-alias` are user-facing CLI commands. `reset` removes this project's registrations from local handoff state; `install-alias` creates a symlink alias for the current binary under `HANDOFF_HOME/bin`.

## JSON Output

Many commands support `--json` for scripting and agent integration. Common examples:

```sh
handoff inbox --json
handoff history --json
handoff sessions --json
handoff profile list --json
handoff context show <context-id> --json
handoff status <job-id> --json
```

## Coding Agent Integrations

This repository includes integration assets:

- `integrations/skills/agent-handoff/SKILL.md`: agent skill instructions.
- `integrations/mcp/agent-handoff-mcp/`: MCP server for tool-based use.
- `docs/`: user guide suitable for GitHub Pages.

### User Guide

The GitHub Pages user guide is published from the `gh-pages` branch:

```text
https://s-hiraoku.github.io/agent-handoff/
```

After editing `docs/`, publish it with:

```sh
make publish-pages
```

### Codex Skill

Copy or symlink the skill into your Codex skills directory:

```sh
mkdir -p ~/.codex/skills
ln -s "$(pwd)/integrations/skills/agent-handoff" ~/.codex/skills/agent-handoff
```

### MCP Server

The MCP server wraps the local `handoff` binary.

```sh
cd integrations/mcp/agent-handoff-mcp
npm install
npm start
```

Example MCP config:

```json
{
  "mcpServers": {
    "agent-handoff": {
      "command": "node",
      "args": ["/absolute/path/to/agent-handoff/integrations/mcp/agent-handoff-mcp/server.js"],
      "env": {
        "HANDOFF_BIN": "handoff",
        "HANDOFF_PROJECT": "/absolute/path/to/project"
      }
    }
  }
}
```

`HANDOFF_PROJECT` pins project identity resolution for MCP clients whose server process may start outside the repository. Each MCP tool also accepts an optional `project` argument; when omitted, the server uses `HANDOFF_PROJECT`, then its current working directory. Tool results keep the CLI JSON text for compatibility and also expose the parsed object as MCP `structuredContent` when the CLI emits JSON.

## Design Notes

`agent-handoff` is local-first by default:

- No account is required.
- No telemetry is sent.
- No remote service is required for MVP operation.
- Messages, context, logs, and job results stay under `HANDOFF_HOME`.

`handoff` is a local execution tool, not a sandbox. Treat message text, context captures, adapter commands, and `--cmd` inputs as trusted local operator input:

- `handoff run` executes shell tasks directly for `shell` runtime.
- `handoff run` and `handoff delegate` may launch local runtime CLIs such as `claude`, `codex`, or `copilot` for built-in runtimes.
- `handoff context create --cmd` and adapter commands run through the local shell.
- MCP clients can trigger the same local CLI behavior through the configured `HANDOFF_BIN`.
- Do not expose the MCP server or adapter environment to untrusted users or unreviewed automated input.

The MVP intentionally uses a structured Rust CLI and SQLite storage instead of a shell-script architecture. SQLite is used as a durable local event store and query model, with schema versioning, foreign key enforcement for new databases, and indexes for common inbox/history/job-log queries.

## Development

Run the full local verification suite:

```sh
./scripts/test.sh
```

The suite runs:

- `cargo fmt --check`
- `cargo check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test`
- CLI smoke tests for identity, messaging, context, jobs, retry, cancellation, log following, and blocked adapters
- MCP smoke tests that call the server through the MCP SDK
- MCP server syntax and dependency checks
- release build

Focused test entry points:

```sh
make fmt
make check
make test
make smoke
make mcp
make release-test
make publish-pages
```

Run only the isolated CLI smoke test:

```sh
./scripts/smoke.sh
```

Run only the MCP checks:

```sh
./scripts/test-mcp.sh
```

## Release

Current release target:

```text
v0.4.0
```

Release checklist:

- `./scripts/test.sh`
- README and docs updated
- tag pushed
- GitHub release created

## License

MIT
