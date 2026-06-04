#!/usr/bin/env node
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";
import { spawn } from "node:child_process";

const HANDOFF_BIN = process.env.HANDOFF_BIN || "handoff";

function runHandoff(args, input) {
  return new Promise((resolve) => {
    const child = spawn(HANDOFF_BIN, args, {
      stdio: ["pipe", "pipe", "pipe"],
      env: process.env
    });

    let stdout = "";
    let stderr = "";

    child.stdout.on("data", (chunk) => {
      stdout += chunk.toString();
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk.toString();
    });
    child.on("error", (error) => {
      resolve({ ok: false, stdout, stderr: String(error), code: -1 });
    });
    child.on("close", (code) => {
      resolve({ ok: code === 0, stdout, stderr, code });
    });

    if (input) {
      child.stdin.write(input);
    }
    child.stdin.end();
  });
}

function textResult(result) {
  const text = result.ok ? result.stdout : `${result.stderr}\n${result.stdout}`.trim();
  return {
    content: [
      {
        type: "text",
        text: text || "(no output)"
      }
    ],
    isError: !result.ok
  };
}

const server = new McpServer({
  name: "agent-handoff",
  version: "0.1.0"
});

server.tool(
  "handoff_send",
  "Send a message to another local agent.",
  {
    agent: z.string(),
    message: z.string(),
    asAgent: z.string().optional(),
    subject: z.string().optional()
  },
  async ({ agent, message, asAgent, subject }) => {
    const args = ["to", agent, message];
    if (asAgent) args.push("--as", asAgent);
    if (subject) args.push("--subject", subject);
    args.push("--json");
    return textResult(await runHandoff(args));
  }
);

server.tool(
  "handoff_inbox",
  "Read the current agent inbox.",
  {
    asAgent: z.string().optional(),
    all: z.boolean().optional(),
    limit: z.number().int().positive().optional()
  },
  async ({ asAgent, all, limit }) => {
    const args = ["inbox", "--json"];
    if (asAgent) args.push("--as", asAgent);
    if (all) args.push("--all");
    if (limit) args.push("--limit", String(limit));
    return textResult(await runHandoff(args));
  }
);

server.tool(
  "handoff_context_create",
  "Create a context package from text, stdin content, git diff, or a command.",
  {
    text: z.string().optional(),
    gitDiff: z.boolean().optional(),
    command: z.string().optional(),
    title: z.string().optional(),
    asAgent: z.string().optional()
  },
  async ({ text, gitDiff, command, title, asAgent }) => {
    const args = ["context", "create", "--json"];
    if (text) args.push("--text", text);
    if (gitDiff) args.push("--git-diff");
    if (command) args.push("--cmd", command);
    if (title) args.push("--title", title);
    if (asAgent) args.push("--as", asAgent);
    return textResult(await runHandoff(args));
  }
);

server.tool(
  "handoff_run",
  "Start a background task for another agent.",
  {
    agent: z.string(),
    task: z.string(),
    contextId: z.string().optional(),
    asAgent: z.string().optional()
  },
  async ({ agent, task, contextId, asAgent }) => {
    const args = ["run", agent, "--task", task, "--json"];
    if (contextId) args.push("--context", contextId);
    if (asAgent) args.push("--as", asAgent);
    return textResult(await runHandoff(args));
  }
);

server.tool(
  "handoff_status",
  "Show one job status or recent jobs.",
  {
    jobId: z.string().optional()
  },
  async ({ jobId }) => {
    const args = ["status", "--json"];
    if (jobId) args.splice(1, 0, jobId);
    return textResult(await runHandoff(args));
  }
);

server.tool(
  "handoff_logs",
  "Show logs for a background job.",
  {
    jobId: z.string(),
    tail: z.number().int().positive().optional()
  },
  async ({ jobId, tail }) => {
    const args = ["logs", jobId, "--json"];
    if (tail) args.push("--tail", String(tail));
    return textResult(await runHandoff(args));
  }
);

server.tool(
  "handoff_result",
  "Show the final result of a completed job.",
  {
    jobId: z.string()
  },
  async ({ jobId }) => {
    return textResult(await runHandoff(["result", jobId, "--json"]));
  }
);

const transport = new StdioServerTransport();
await server.connect(transport);

