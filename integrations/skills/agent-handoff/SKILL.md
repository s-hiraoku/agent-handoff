---
name: agent-handoff
description: Use handoff to communicate with other coding agents, pass context, and start background agent tasks.
---

# agent-handoff

Use this skill when the user wants to send information to another coding agent, check messages from other agents, hand off context, or run a background task through `handoff`.

## Requirements

- The `handoff` binary must be installed and available on `PATH`.
- The current project should be joined with `handoff join <team> <agent>`.
- If multiple identities exist, use `handoff actas <agent>` or pass `--as <agent>`.

## Core Workflow

Check identity:

```sh
handoff whoami
handoff active
handoff actas <agent>
```

Check inbox before starting coordinated work:

```sh
handoff inbox
```

Preview inbox without consuming unread messages:

```sh
handoff inbox --peek
```

Send a concise message:

```sh
handoff to <agent> "message"
```

Send command output:

```sh
some-command 2>&1 | handoff to <agent> --stdin --subject "command output"
```

Create context:

```sh
handoff context create --git-diff
handoff context create --file <path>
handoff context create --cmd "command"
```

Send context:

```sh
handoff to <agent> --context <context-id> --message "Use this context."
handoff to <agent> --git-diff --message "Review this diff."
handoff to <agent> --file <path> --as-context --message "Use this file as context."
```

Run background work:

```sh
handoff run <agent> --task "task text" --context <context-id>
handoff run <agent> --task "task text" --timeout 30
handoff status <job-id>
handoff logs <job-id>
handoff result <job-id>
```

## Agent Behavior

- Prefer `--json` when parsing command output programmatically.
- For MCP use, set `HANDOFF_PROJECT` or pass the MCP tool `project` argument so identities resolve against the intended repository.
- Keep messages short unless sending context.
- Use `handoff context create` for large diffs, files, or command output.
- Use `handoff reply <thread-id>` when responding to an existing handoff.
- Use `handoff monitor --as <agent> --once` for a scriptable one-shot delivery check, or `handoff mode monitor --runtime claude-code` when the host supports persistent Monitor streams.
- Use `handoff inbox --peek` when checking whether work exists without marking messages read.
- Before assuming another agent ignored a task, check `handoff status` and `handoff logs`.
- If a job is `blocked`, report the adapter/runtime limitation clearly.

## Useful Commands

```sh
handoff agents
handoff history
handoff context list
handoff status
```
