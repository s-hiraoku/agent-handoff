---
layout: handoff
title: Workflow Playbook
description: Practical setup and daily workflow guide for agent-handoff.
---

<section class="hero">
  <div class="site-shell hero-grid">
    <div>
      <p class="eyebrow">Local-first agent coordination</p>
      <h1>Workflow Playbook</h1>
      <p class="lead">
        Use agent-handoff as a practical runbook for coding-agent work: create identities,
        pass context, check inboxes safely, run background jobs, and recover from blocked tasks.
      </p>
      <div class="hero-actions">
        <a class="button primary" href="#daily-loop">Start daily loop</a>
        <a class="button" href="{{ '/reference/' | relative_url }}">Command reference</a>
      </div>
      <div class="status-strip" aria-label="Project status">
        <div class="status-chip">
          <strong>Local-first</strong>
          <span>State stays under HANDOFF_HOME</span>
        </div>
        <div class="status-chip">
          <strong>CLI + MCP</strong>
          <span>Use shell or tool integrations</span>
        </div>
        <div class="status-chip">
          <strong>Safe inbox</strong>
          <span>Peek before marking read</span>
        </div>
      </div>
    </div>

    <aside class="runbook-panel" aria-label="Quick runbook">
      <div class="panel-head">
        <div>
          <h2>Runbook</h2>
          <p>Minimal commands for one project.</p>
        </div>
        <span class="phase-badge">setup</span>
      </div>
      <div class="runbook-list">
        <div class="runbook-row">
          <span>install</span>
          <code>cargo install --path .</code>
        </div>
        <div class="runbook-row">
          <span>session</span>
          <code>handoff session alias lead</code>
        </div>
        <div class="runbook-row">
          <span>profile</span>
          <code>handoff profile create reviewer --runtime shell</code>
        </div>
        <div class="runbook-row">
          <span>peek</span>
          <code>handoff inbox --peek</code>
        </div>
      </div>
    </aside>
  </div>
</section>

<section class="workflow-band" aria-labelledby="workflow-title">
  <div class="site-shell">
    <h2 id="workflow-title">Daily Loop</h2>
    <div class="flow-strip">
      <div class="flow-step">
        <span>1</span>
        <strong>Alias Session</strong>
      </div>
      <div class="flow-step">
        <span>2</span>
        <strong>Create Profile</strong>
      </div>
      <div class="flow-step">
        <span>3</span>
        <strong>Send</strong>
      </div>
      <div class="flow-step">
        <span>4</span>
        <strong>Peek Inbox</strong>
      </div>
      <div class="flow-step">
        <span>5</span>
        <strong>Run Job</strong>
      </div>
      <div class="flow-step">
        <span>6</span>
        <strong>Review Result</strong>
      </div>
    </div>
  </div>
</section>

<section id="daily-loop" class="section-band">
  <div class="site-shell content-grid">
    <div>
      <h2>First Handoff</h2>
      <div class="steps">
        <article class="step-item">
          <span class="step-index">1</span>
          <div>
            <h3>Create sessions and profiles</h3>
            <p>Give live sessions readable aliases and create project or user-global profiles for delegated work.</p>
            <pre class="command-block"><code>handoff init
handoff session alias lead
HANDOFF_SESSION_ID=reviewer-session handoff session alias reviewer
handoff profile create reviewer --runtime shell
handoff profile set reviewer session=@reviewer capability=review</code></pre>
          </div>
        </article>
        <article class="step-item">
          <span class="step-index">2</span>
          <div>
            <h3>Send work</h3>
            <p>The sender is the current session. Use <code>@alias</code> for live session inbox delivery.</p>
            <pre class="command-block"><code>handoff to @reviewer "Please review this change."
handoff route --capability review "Please review this change."</code></pre>
          </div>
        </article>
        <article class="step-item">
          <span class="step-index">3</span>
          <div>
            <h3>Read safely</h3>
            <p>Preview messages before consuming unread state, then read normally when ready.</p>
            <pre class="command-block"><code>HANDOFF_SESSION_ID=reviewer-session handoff inbox --peek
handoff inbox</code></pre>
          </div>
        </article>
      </div>
    </div>

    <aside class="side-stack" aria-label="Operational notes">
      <div class="note-panel">
        <strong>Current session</strong>
        <p><code>handoff session alias &lt;alias&gt;</code> gives the current live session a readable <code>@alias</code> address.</p>
      </div>
      <div class="note-panel">
        <strong>JSON mode</strong>
        <p>Use <code>--json</code> for scripts, hooks, and MCP integrations.</p>
      </div>
      <div class="note-panel">
        <strong>Reference</strong>
        <p>Use the command table when you need flags, outputs, or recovery hints.</p>
        <div class="inline-actions">
          <a class="button" href="{{ '/reference/' | relative_url }}">Open reference</a>
        </div>
      </div>
    </aside>
  </div>
</section>

<section class="section-band" aria-labelledby="context-title">
  <div class="site-shell content-grid">
    <div>
      <h2 id="context-title">Context and Jobs</h2>
      <div class="steps">
        <article class="step-item">
          <span class="step-index">4</span>
          <div>
            <h3>Package context</h3>
            <p>Capture diffs, files, stdin, text, or command output as a reusable context package.</p>
            <pre class="command-block"><code>CTX=$(handoff context create --git-diff --json | jq -r .context_id)
handoff to @reviewer --context "$CTX" --message "Use this diff."</code></pre>
          </div>
        </article>
        <article class="step-item">
          <span class="step-index">5</span>
          <div>
            <h3>Run background work</h3>
            <p>Use <code>delegate --wait</code> when the caller needs the result immediately. Use <code>run</code> when background execution is enough.</p>
            <pre class="command-block"><code>git diff | handoff delegate reviewer --stdin --task "Review this diff" --wait</code></pre>
            <p>Shell runtime executes the task text. <code>claude-code</code>, <code>codex</code>, and <code>copilot</code> have built-in adapters; other runtimes need adapter commands.</p>
            <pre class="command-block"><code>handoff run reviewer --task 'echo reviewed: "$HANDOFF_TASK"' --context "$CTX"
handoff status &lt;job-id&gt;
handoff logs &lt;job-id&gt; --follow
handoff result &lt;job-id&gt;</code></pre>
          </div>
        </article>
        <article class="step-item">
          <span class="step-index">6</span>
          <div>
            <h3>Recover blocked jobs</h3>
            <p>Configure per-agent or per-runtime adapters, then retry the job.</p>
            <pre class="command-block"><code>export HANDOFF_AGENT_CMD_REVIEWER='reviewer-agent --task "$HANDOFF_TASK"'
handoff retry &lt;job-id&gt;</code></pre>
          </div>
        </article>
      </div>
    </div>

    <aside class="side-stack">
      <div class="note-panel">
        <strong>Adapter environment</strong>
        <p>Jobs receive <code>HANDOFF_JOB_ID</code>, <code>HANDOFF_TASK</code>, <code>HANDOFF_CONTEXT</code>, and <code>HANDOFF_PROMPT</code>.</p>
      </div>
      <div class="note-panel">
        <strong>Trust boundary</strong>
        <p>Shell jobs, command captures, and adapters execute locally; use them only with trusted input.</p>
      </div>
      <div class="note-panel">
        <strong>Delivery modes</strong>
        <p>Use <code>handoff mode turn --runtime codex</code>, <code>handoff mode turn --runtime copilot</code>, or <code>handoff mode both --runtime claude-code</code> for hooks. Turn hooks call <code>handoff notify --hook --json</code> so incoming messages are surfaced as actionable agent context.</p>
      </div>
    </aside>
  </div>
</section>

<section class="section-band" aria-labelledby="mcp-title">
  <div class="site-shell content-grid">
    <div>
      <h2 id="mcp-title">MCP Integration</h2>
      <p class="lead">
        Pin project resolution when an MCP host may start outside the repository. Tool-level
        <code>project</code> takes precedence over <code>HANDOFF_PROJECT</code>; otherwise the server uses its cwd.
      </p>
      <pre class="command-block"><code>{
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
}</code></pre>
    </div>
    <aside class="side-stack">
      <div class="note-panel">
        <strong>Tools</strong>
        <p>Send, reply, inbox, history, context, jobs, logs, result, cancel, and retry. JSON results are also exposed as MCP structured content.</p>
      </div>
      <div class="note-panel">
        <strong>Troubleshooting</strong>
        <p><code>unknown_session</code> means run <code>handoff sessions</code> or assign an alias with <code>handoff session alias &lt;alias&gt;</code>. If a profile and session share a name, use <code>@alias</code> for inbox delivery.</p>
      </div>
    </aside>
  </div>
</section>

<section class="section-band" aria-labelledby="direction-title">
  <div class="site-shell">
    <h2 id="direction-title">Design Direction</h2>
    <figure class="snapshot">
      <img src="{{ '/assets/img/workflow-playbook-concept.png' | relative_url }}" alt="Workflow Playbook user guide design direction mockup">
      <figcaption>
        The guide follows a workflow-first structure, with the command reference separated into a searchable lower-level page.
      </figcaption>
    </figure>
  </div>
</section>
