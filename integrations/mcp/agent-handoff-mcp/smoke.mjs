import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

function parseToolJson(result) {
  const text = result.content?.[0]?.text;
  if (!text) {
    throw new Error("MCP tool returned no text content");
  }
  const parsed = JSON.parse(text);
  if (!parsed.ok) {
    throw new Error(`MCP tool returned ok=false: ${text}`);
  }
  if (JSON.stringify(result.structuredContent) !== JSON.stringify(parsed)) {
    throw new Error(
      `MCP structuredContent did not match text JSON: ${JSON.stringify(result.structuredContent)}`,
    );
  }
  return parsed;
}

const project = process.env.HANDOFF_PROJECT;
const handoffBin = process.env.HANDOFF_BIN;

if (!project || !handoffBin) {
  throw new Error("HANDOFF_PROJECT and HANDOFF_BIN are required");
}

const transport = new StdioClientTransport({
  command: "node",
  args: [path.join(__dirname, "server.js")],
  env: {
    ...process.env,
    HANDOFF_BIN: handoffBin,
    HANDOFF_PROJECT: project,
  },
});

const client = new Client({
  name: "agent-handoff-mcp-smoke",
  version: "0.2.0",
});

await client.connect(transport);

try {
  const context = parseToolJson(
    await client.callTool({
      name: "handoff_context_create",
      arguments: {
        text: "mcp context",
        title: "MCP smoke",
        asAgent: "lead",
      },
    }),
  );

  const send = parseToolJson(
    await client.callTool({
      name: "handoff_send",
      arguments: {
        agent: "@reviewer",
        asAgent: "lead",
        message: "MCP smoke message",
        contextId: context.context_id,
      },
    }),
  );

  if (!send.message_id) {
    throw new Error("handoff_send did not return message_id");
  }

  const inbox = parseToolJson(
    await client.callTool({
      name: "handoff_inbox",
      arguments: {
        sessionId: "reviewer-session",
        peek: true,
        project,
      },
    }),
  );

  if (inbox.messages.length !== 1 || inbox.messages[0].body !== "MCP smoke message") {
    throw new Error(`unexpected inbox result: ${JSON.stringify(inbox)}`);
  }

  const routed = parseToolJson(
    await client.callTool({
      name: "handoff_route",
      arguments: {
        capability: "review",
        message: "MCP routed message",
        project,
      },
    }),
  );

  if (!routed.message_id) {
    throw new Error("handoff_route did not return message_id");
  }

  const routedInbox = parseToolJson(
    await client.callTool({
      name: "handoff_inbox",
      arguments: {
        sessionId: "reviewer-session",
        peek: true,
        project,
      },
    }),
  );

  if (routedInbox.messages.length !== 2 || routedInbox.messages[1].body !== "MCP routed message") {
    throw new Error(`unexpected routed inbox result: ${JSON.stringify(routedInbox)}`);
  }

  const delegated = parseToolJson(
    await client.callTool({
      name: "handoff_delegate",
      arguments: {
        agent: "reviewer",
        asAgent: "lead",
        task: 'printf "mcp delegated: %s\\n" "$HANDOFF_TASK"',
        wait: true,
        timeout: 5,
        project,
      },
    }),
  );

  if (!delegated.result?.body?.includes("mcp delegated:")) {
    throw new Error(`unexpected delegate result: ${JSON.stringify(delegated)}`);
  }
} finally {
  await client.close();
}
