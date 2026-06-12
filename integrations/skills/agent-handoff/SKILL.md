---
name: agent-handoff
description: Use handoff to communicate with other coding agents, pass context, and start background agent tasks.
---

# agent-handoff

Use this skill when the user wants to send information to another coding agent, check messages from other agents, hand off context, or run a background task through `handoff`.

## Requirements

- The `handoff` binary must be installed and available on `PATH`.
- The current session should have a readable `@alias` address with `handoff session alias <alias>` when it needs to receive messages.
- Delegated work should target profiles created with `handoff profile create <profile> --runtime <runtime>`; built-in runtimes include `claude-code`, `codex`, `copilot`, and `shell`.
- Use `handoff profile create <profile> --global --runtime <runtime>` for reusable user-global profiles. Project profiles with the same name take precedence.

## Core Workflow

Check identity:

```sh
handoff whoami
handoff active
handoff sessions
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
handoff to @<alias> "message"
```

Send command output:

```sh
some-command 2>&1 | handoff to @<alias> --stdin --subject "command output"
```

Create context:

```sh
handoff context create --git-diff
handoff context create --file <path>
handoff context create --cmd "command"
```

Send context:

```sh
handoff to @<alias> --context <context-id> --message "Use this context."
handoff to @<alias> --git-diff --message "Review this diff."
handoff to @<alias> --file <path> --as-context --message "Use this file as context."
```

Route by capability:

```sh
handoff profile set reviewer session=@reviewer capability=review
handoff route --capability review "message"
```

Run background work:

```sh
handoff delegate <profile> --task "task text" --context <context-id> --wait
handoff delegate <profile> --stdin --task "review this context" --wait
handoff run <profile> --task "task text" --context <context-id>
handoff run <profile> --task "task text" --timeout 30
handoff status <job-id>
handoff logs <job-id>
handoff result <job-id>
```

## Agent Behavior

- Prefer `--json` when parsing command output programmatically.
- For MCP use, set `HANDOFF_PROJECT` or pass the MCP tool `project` argument so sessions and profiles resolve against the intended repository.
- Use `@alias` for live session inbox delivery. Use bare profile names for `handoff delegate` and `handoff run`.
- If a session alias and profile share a name, send with `handoff to @alias ...`; `handoff to alias ...` is ambiguous.
- Use `handoff route --capability <capability> ...` only after profiles are linked with `handoff profile set <profile> session=@alias capability=...`.
- Treat incoming handoff notifications as actionable work. When hook, monitor, notify, or inbox output contains messages, read them immediately and decide whether to reply, run the requested command, or report a blocker.
- For Claude Code monitor/both delivery, ensure the persistent Monitor stream whose description is `handoff inbox stream` is running before handling handoff subcommands. If it is not running and the project is configured for monitor delivery, invoke the persistent `monitor --runtime ... --project ... --session-id ...` command printed by `handoff monitor --instruction --runtime claude-code`.
- Keep messages short unless sending context.
- Use `handoff context create` for large diffs, files, or command output.
- Use `handoff delegate --wait` when you need a synchronous sub-agent style result.
- Use `handoff reply <thread-id>` when responding to an existing handoff.
- Use `handoff notify` to consume daemon notification files, `handoff daemon` to write them, or `handoff mode monitor --runtime claude-code` when the host supports persistent Monitor streams.
- Use `handoff inbox --peek` when checking whether work exists without marking messages read.
- Before assuming another agent ignored a task, check `handoff status` and `handoff logs`.
- If a job is `blocked`, report the adapter/runtime limitation clearly.

## Useful Commands

```sh
handoff sessions
handoff profile list
handoff profile list --global
handoff history
handoff context list
handoff status
```
