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

## MCP Config

```json
{
  "mcpServers": {
    "agent-handoff": {
      "command": "node",
      "args": ["/absolute/path/to/agent-handoff/integrations/mcp/agent-handoff-mcp/server.js"],
      "env": {
        "HANDOFF_BIN": "handoff"
      }
    }
  }
}
```

## Tools

- `handoff_send`
- `handoff_inbox`
- `handoff_context_create`
- `handoff_run`
- `handoff_status`
- `handoff_logs`
- `handoff_result`

