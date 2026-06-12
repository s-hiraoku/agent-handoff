# agent-handoff MCP Server

This MCP server exposes the local `handoff` CLI to coding agents.

## Install

```sh
cd integrations/mcp/agent-handoff-mcp
npm install
npm start
```

Set `HANDOFF_BIN` if `handoff` is not on `PATH`:

```sh
HANDOFF_BIN=/absolute/path/to/handoff npm start
```

Set `HANDOFF_PROJECT` when the MCP host may start the server outside your repository:

```sh
HANDOFF_PROJECT=/absolute/path/to/project npm start
```

Each tool also accepts an optional `project` argument. Tool-level `project` takes precedence over `HANDOFF_PROJECT`; otherwise the server uses its current working directory.

The wrapped CLI infers the sending session from `HANDOFF_SESSION_ID`, `CLAUDE_CODE_SESSION_ID`, `CODEX_SESSION_ID`, or a project-local fallback session. `handoff_inbox` also accepts `sessionId` to read a specific live session. Message tools address sessions with `@alias` or session IDs; `handoff_run` and `handoff_delegate` target delegation profiles.

Tool results preserve the CLI JSON text for compatibility and also expose the parsed JSON as MCP `structuredContent` when the wrapped CLI command emits JSON.

## MCP Config

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

## Tools

- `handoff_send`
- `handoff_route`
- `handoff_inbox`
- `handoff_context_create`
- `handoff_context_file`
- `handoff_context_show`
- `handoff_reply`
- `handoff_history`
- `handoff_show`
- `handoff_run`
- `handoff_delegate`
- `handoff_status`
- `handoff_logs`
- `handoff_result`
- `handoff_cancel`
- `handoff_retry`

## Trust Boundary

This server wraps a local CLI and inherits its execution behavior. `handoff_run`, `handoff_delegate`, `handoff_context_create` with `command`, built-in runtime adapters, and configured adapter commands may execute local commands through `HANDOFF_BIN`. Run the MCP server only for trusted clients and trusted projects; do not expose it as a network service for untrusted users.

## Test

From the repository root:

```sh
./scripts/test-mcp.sh
```

The smoke test starts the MCP server with an isolated `HANDOFF_HOME`, calls tools through the MCP SDK, and verifies context creation, sending, inbox reads, and synchronous delegation.
