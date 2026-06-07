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
- `handoff_inbox`
- `handoff_context_create`
- `handoff_context_file`
- `handoff_context_show`
- `handoff_reply`
- `handoff_history`
- `handoff_show`
- `handoff_run`
- `handoff_status`
- `handoff_logs`
- `handoff_result`
- `handoff_cancel`
- `handoff_retry`
