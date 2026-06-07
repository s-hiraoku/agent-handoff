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

MVP implementation. The storage format and CLI may still change before `v1.0`, but the current release is usable for local agent-to-agent messaging, context passing, and shell-backed background tasks.

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

Create two local agent identities in the same project:

```sh
handoff init
handoff join demo lead --runtime shell
handoff join demo reviewer --runtime shell
```

Select the sender:

```sh
handoff actas lead
```

Send a message:

```sh
handoff to reviewer "Please review the current diff."
```

Read as the recipient:

```sh
handoff actas reviewer
handoff inbox
```

Preview without marking messages read:

```sh
handoff inbox --peek
```

Send command output or a file:

```sh
git diff | handoff to reviewer --stdin --subject "Current diff"
handoff to reviewer --file notes.md
```

Create and send context:

```sh
handoff actas lead
CTX=$(handoff context create --git-diff --json | jq -r .context_id)
handoff to reviewer --context "$CTX" --message "Use this diff as context."
handoff to reviewer --git-diff --message "Review this diff."
handoff to reviewer --file notes.md --as-context --message "Use these notes."
handoff context show "$CTX"
```

Run a background task:

```sh
handoff run reviewer --task 'echo reviewed: "$HANDOFF_TASK"'
handoff status
handoff logs <job-id>
handoff result <job-id>
```

`handoff status <job-id>` prints a short human-readable summary by default. Use `--json` when scripting.

For `shell` runtime, the task text is executed by the shell. For other runtimes, set an adapter command:

```sh
export HANDOFF_AGENT_CMD_REVIEWER='my-reviewer-agent --task "$HANDOFF_TASK"'
handoff run reviewer --task "Review the diff" --context "$CTX"
```

The spawned process receives:

```text
HANDOFF_JOB_ID
HANDOFF_TASK
HANDOFF_CONTEXT
```

## Core Commands

Identity and teams:

```sh
handoff join <team> <agent> --runtime shell
handoff whoami
handoff actas <agent>
handoff active
handoff drop <agent>
handoff agents
handoff rename-team <old> <new>
```

When the host runtime provides `HANDOFF_SESSION_ID`, `CLAUDE_CODE_SESSION_ID`, or `CODEX_SESSION_ID`, `actas` claims a lease for that role so another live session cannot accidentally consume the same agent's messages. Without a stable session id, `actas` still selects the active role with a project-local lease.

Messaging:

```sh
handoff send <agent> <message>
handoff to <agent> <message>
handoff post <agent> <message>
handoff reply <thread-id> <message>
handoff inbox
handoff history
handoff show <message-id>
```

Context:

```sh
handoff context create --text "notes"
handoff context create --stdin
handoff context create --file notes.md
handoff context create --git-diff
handoff context create --cmd "cargo test"
handoff context show <context-id>
handoff context list
```

Jobs:

```sh
handoff run <agent> --task <text>
handoff run <agent> --task <text> --timeout 30
handoff status [job-id]
handoff logs <job-id>
handoff result <job-id>
handoff cancel <job-id>
handoff retry <job-id>
```

Delivery mode:

```sh
handoff mode
handoff mode turn
handoff mode monitor --runtime claude-code
handoff mode both --runtime claude-code
handoff mode off
handoff monitor --as reviewer --runtime shell
```

`mode turn` writes a project-local inbox hook for supported runtimes. `mode monitor` writes a Claude Code `SessionStart` hook that asks the host to launch a persistent `handoff monitor` stream. `mode both` installs both delivery paths. `mode off` removes handoff-owned hook entries.

## JSON Output

Read commands support `--json` for scripting and agent integration:

```sh
handoff inbox --json
handoff history --json
handoff agents --json
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

`HANDOFF_PROJECT` pins project identity resolution for MCP clients whose server process may start outside the repository. Each MCP tool also accepts an optional `project` argument; when omitted, the server uses `HANDOFF_PROJECT`, then its current working directory.

## Design Notes

`agent-handoff` is local-first by default:

- No account is required.
- No telemetry is sent.
- No remote service is required for MVP operation.
- Messages, context, logs, and job results stay under `HANDOFF_HOME`.

The MVP intentionally uses a structured Rust CLI and SQLite storage instead of a shell-script architecture. SQLite is used as a durable local event store and query model.

## Development

Run the full local verification suite:

```sh
./scripts/test.sh
```

The suite runs:

- `cargo fmt --check`
- `cargo check`
- `cargo test`
- CLI smoke tests for identity, messaging, context, jobs, retry, and blocked adapters
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
v0.1.0
```

Release checklist:

- `./scripts/test.sh`
- README and docs updated
- tag pushed
- GitHub release created

## License

MIT
