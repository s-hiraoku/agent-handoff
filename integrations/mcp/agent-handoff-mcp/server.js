#!/usr/bin/env node
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";
import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import path from "node:path";

const HANDOFF_BIN = process.env.HANDOFF_BIN || "handoff";
const DEFAULT_HANDOFF_TIMEOUT_MS = 30000;
const DEFAULT_DELEGATE_WAIT_TIMEOUT_SECONDS = 300;
const DELEGATE_WAIT_BUFFER_MS = 5000;
const packageJson = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8"));

function projectCwd(project) {
  return path.resolve(project || process.env.HANDOFF_PROJECT || process.cwd());
}

function handoffTimeoutMs(overrideMs) {
  const timeoutMs = Number(overrideMs || process.env.HANDOFF_MCP_TIMEOUT_MS || DEFAULT_HANDOFF_TIMEOUT_MS);
  return Number.isFinite(timeoutMs) && timeoutMs > 0 ? timeoutMs : DEFAULT_HANDOFF_TIMEOUT_MS;
}

function runHandoff(args, input, project, options = {}) {
  return new Promise((resolve) => {
    const child = spawn(HANDOFF_BIN, args, {
      stdio: ["pipe", "pipe", "pipe"],
      env: process.env,
      cwd: projectCwd(project)
    });
    const timeoutMs = handoffTimeoutMs(options.timeoutMs);

    let stdout = "";
    let stderr = "";
    let settled = false;
    let timer;
    const finish = (result) => {
      if (settled) {
        return;
      }
      settled = true;
      clearTimeout(timer);
      resolve(result);
    };
    timer = setTimeout(() => {
      child.kill("SIGKILL");
      const timeoutMessage = `handoff timed out after ${timeoutMs}ms`;
      finish({
        ok: false,
        stdout,
        stderr: stderr ? `${stderr}\n${timeoutMessage}` : timeoutMessage,
        code: -2
      });
    }, timeoutMs);

    child.stdout.on("data", (chunk) => {
      stdout += chunk.toString();
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk.toString();
    });
    child.on("error", (error) => {
      finish({ ok: false, stdout, stderr: String(error), code: -1 });
    });
    child.on("close", (code) => {
      finish({ ok: code === 0, stdout, stderr, code });
    });

    if (input) {
      child.stdin.write(input);
    }
    child.stdin.end();
  });
}

function textResult(result) {
  const text = result.ok ? result.stdout : `${result.stderr}\n${result.stdout}`.trim();
  const structuredContent = parseStructuredContent(result, text);
  return {
    content: [
      {
        type: "text",
        text: text || "(no output)"
      }
    ],
    structuredContent,
    _meta: {
      handoffExitCode: result.code
    },
    isError: !result.ok
  };
}

function parseStructuredContent(result, text) {
  if (result.ok) {
    try {
      return JSON.parse(result.stdout);
    } catch {
      return {
        ok: true,
        output: text
      };
    }
  }
  return {
    ok: false,
    code: result.code,
    stdout: result.stdout,
    stderr: result.stderr
  };
}

const server = new McpServer({
  name: "agent-handoff",
  version: packageJson.version
});

server.tool(
  "handoff_send",
  "Send a message to another live session using @alias or a session id.",
  {
    agent: z.string(),
    message: z.string(),
    asAgent: z.string().optional(),
    subject: z.string().optional(),
    team: z.string().optional(),
    threadId: z.string().optional(),
    contextId: z.string().optional(),
    project: z.string().optional()
  },
  async ({ agent, message, asAgent, subject, team, threadId, contextId, project }) => {
    const args = ["to", agent, message];
    if (asAgent) args.push("--as", asAgent);
    if (subject) args.push("--subject", subject);
    if (team) args.push("--team", team);
    if (threadId) args.push("--thread", threadId);
    if (contextId) args.push("--context", contextId);
    args.push("--json");
    return textResult(await runHandoff(args, undefined, project));
  }
);

server.tool(
  "handoff_route",
  "Route a message to an active session linked from a capable profile.",
  {
    capability: z.string(),
    message: z.string().optional(),
    subject: z.string().optional(),
    project: z.string().optional()
  },
  async ({ capability, message, subject, project }) => {
    const args = ["route", "--capability", capability, "--json"];
    if (subject) args.push("--subject", subject);
    if (message) args.push(message);
    return textResult(await runHandoff(args, undefined, project));
  }
);

server.tool(
  "handoff_inbox",
  "Read the current agent inbox.",
  {
    asAgent: z.string().optional(),
    sessionId: z.string().optional(),
    all: z.boolean().optional(),
    peek: z.boolean().optional(),
    limit: z.number().int().positive().optional(),
    project: z.string().optional()
  },
  async ({ asAgent, sessionId, all, peek, limit, project }) => {
    const args = ["inbox", "--json"];
    if (sessionId) args.push("--session-id", sessionId);
    if (asAgent) args.push("--as", asAgent);
    if (all) args.push("--all");
    if (peek) args.push("--peek");
    if (limit) args.push("--limit", String(limit));
    return textResult(await runHandoff(args, undefined, project));
  }
);

server.tool(
  "handoff_context_create",
  "Create a context package from text, stdin content, git diff, or a command.",
  {
    text: z.string().optional(),
    stdin: z.string().optional(),
    gitDiff: z.boolean().optional(),
    command: z.string().optional(),
    file: z.string().optional(),
    files: z.array(z.string()).optional(),
    title: z.string().optional(),
    asAgent: z.string().optional(),
    project: z.string().optional()
  },
  async ({ text, stdin, gitDiff, command, file, files, title, asAgent, project }) => {
    const args = ["context", "create", "--json"];
    if (text) args.push("--text", text);
    if (stdin) args.push("--stdin");
    if (gitDiff) args.push("--git-diff");
    if (command) args.push("--cmd", command);
    if (file) args.push("--file", file);
    if (files) {
      for (const item of files) args.push("--files", item);
    }
    if (title) args.push("--title", title);
    if (asAgent) args.push("--as", asAgent);
    return textResult(await runHandoff(args, stdin, project));
  }
);

server.tool(
  "handoff_run",
  "Start a background task for another agent.",
  {
    agent: z.string(),
    task: z.string(),
    contextId: z.string().optional(),
    asAgent: z.string().optional(),
    timeout: z.number().int().positive().optional(),
    project: z.string().optional()
  },
  async ({ agent, task, contextId, asAgent, timeout, project }) => {
    const args = ["run", agent, "--task", task, "--json"];
    if (contextId) args.push("--context", contextId);
    if (asAgent) args.push("--as", asAgent);
    if (timeout) args.push("--timeout", String(timeout));
    return textResult(await runHandoff(args, undefined, project));
  }
);

server.tool(
  "handoff_delegate",
  "Delegate a task to another agent and optionally wait for the result.",
  {
    agent: z.string(),
    task: z.string().optional(),
    stdin: z.string().optional(),
    gitDiff: z.boolean().optional(),
    file: z.string().optional(),
    contextId: z.string().optional(),
    wait: z.boolean().optional(),
    asAgent: z.string().optional(),
    subject: z.string().optional(),
    timeout: z.number().int().positive().optional(),
    waitTimeout: z.number().int().positive().optional(),
    project: z.string().optional()
  },
  async ({ agent, task, stdin, gitDiff, file, contextId, wait, asAgent, subject, timeout, waitTimeout, project }) => {
    const args = ["delegate", agent, "--json"];
    if (task) args.push("--task", task);
    if (stdin) args.push("--stdin");
    if (gitDiff) args.push("--git-diff");
    if (file) args.push("--file", file);
    if (contextId) args.push("--context", contextId);
    if (wait) args.push("--wait");
    if (asAgent) args.push("--as", asAgent);
    if (subject) args.push("--subject", subject);
    if (timeout) args.push("--timeout", String(timeout));
    if (waitTimeout) args.push("--wait-timeout", String(waitTimeout));
    const waitSeconds = waitTimeout || (timeout ? timeout + 5 : DEFAULT_DELEGATE_WAIT_TIMEOUT_SECONDS);
    const timeoutMs = wait ? (waitSeconds * 1000) + DELEGATE_WAIT_BUFFER_MS : undefined;
    return textResult(await runHandoff(args, stdin, project, { timeoutMs }));
  }
);

server.tool(
  "handoff_status",
  "Show one job status or recent jobs.",
  {
    jobId: z.string().optional(),
    project: z.string().optional()
  },
  async ({ jobId, project }) => {
    const args = ["status", "--json"];
    if (jobId) args.splice(1, 0, jobId);
    return textResult(await runHandoff(args, undefined, project));
  }
);

server.tool(
  "handoff_doctor",
  "Diagnose handoff setup for the configured project.",
  {
    project: z.string().optional()
  },
  async ({ project }) => {
    return textResult(await runHandoff(["doctor", "--json"], undefined, project));
  }
);

server.tool(
  "handoff_logs",
  "Show logs for a background job.",
  {
    jobId: z.string(),
    tail: z.number().int().positive().optional(),
    project: z.string().optional()
  },
  async ({ jobId, tail, project }) => {
    const args = ["logs", jobId, "--json"];
    if (tail) args.push("--tail", String(tail));
    return textResult(await runHandoff(args, undefined, project));
  }
);

server.tool(
  "handoff_result",
  "Show the final result of a completed job.",
  {
    jobId: z.string(),
    project: z.string().optional()
  },
  async ({ jobId, project }) => {
    return textResult(await runHandoff(["result", jobId, "--json"], undefined, project));
  }
);

server.tool(
  "handoff_reply",
  "Reply to an existing handoff thread.",
  {
    threadId: z.string(),
    message: z.string(),
    asAgent: z.string().optional(),
    subject: z.string().optional(),
    project: z.string().optional()
  },
  async ({ threadId, message, asAgent, subject, project }) => {
    const args = ["reply", threadId, message, "--json"];
    if (asAgent) args.push("--as", asAgent);
    if (subject) args.push("--subject", subject);
    return textResult(await runHandoff(args, undefined, project));
  }
);

server.tool(
  "handoff_history",
  "Show recent message history for the active team.",
  {
    withAgent: z.string().optional(),
    team: z.string().optional(),
    limit: z.number().int().positive().optional(),
    project: z.string().optional()
  },
  async ({ withAgent, team, limit, project }) => {
    const args = ["history", "--json"];
    if (withAgent) args.push("--with", withAgent);
    if (team) args.push("--team", team);
    if (limit) args.push("--limit", String(limit));
    return textResult(await runHandoff(args, undefined, project));
  }
);

server.tool(
  "handoff_show",
  "Show a message by id.",
  {
    messageId: z.string(),
    project: z.string().optional()
  },
  async ({ messageId, project }) => {
    return textResult(await runHandoff(["show", messageId, "--json"], undefined, project));
  }
);

server.tool(
  "handoff_context_show",
  "Show a context package by id.",
  {
    contextId: z.string(),
    project: z.string().optional()
  },
  async ({ contextId, project }) => {
    return textResult(await runHandoff(["context", "show", contextId, "--json"], undefined, project));
  }
);

server.tool(
  "handoff_context_file",
  "Create a context package from one file.",
  {
    file: z.string(),
    title: z.string().optional(),
    asAgent: z.string().optional(),
    project: z.string().optional()
  },
  async ({ file, title, asAgent, project }) => {
    const args = ["context", "create", "--file", file, "--json"];
    if (title) args.push("--title", title);
    if (asAgent) args.push("--as", asAgent);
    return textResult(await runHandoff(args, undefined, project));
  }
);

server.tool(
  "handoff_cancel",
  "Cancel a queued or running background job.",
  {
    jobId: z.string(),
    project: z.string().optional()
  },
  async ({ jobId, project }) => {
    return textResult(await runHandoff(["cancel", jobId], undefined, project));
  }
);

server.tool(
  "handoff_retry",
  "Retry a background job.",
  {
    jobId: z.string(),
    project: z.string().optional()
  },
  async ({ jobId, project }) => {
    return textResult(await runHandoff(["retry", jobId, "--json"], undefined, project));
  }
);

const transport = new StdioServerTransport();
await server.connect(transport);
