---
layout: handoff
title: Reference Console
description: Searchable command reference for agent-handoff.
permalink: /reference/
reference_search: true
---

<section class="reference-hero">
  <div class="site-shell">
    <p class="eyebrow">Command reference</p>
    <h1>Reference Console</h1>
    <p class="lead">
      Search commands by workflow area, inspect flags and output shape, and keep recovery
      commands close while operating agent-handoff in a development environment.
    </p>
    <div class="reference-toolbar" role="search">
      <label>
        <span class="eyebrow">Search</span>
        <input class="search-box" type="search" data-command-search placeholder="Search commands, flags, outputs, recovery">
      </label>
      <div class="tab-list" aria-label="Command categories">
        <button class="tab-button" type="button" data-command-filter="all" aria-pressed="true">All</button>
        <button class="tab-button" type="button" data-command-filter="messaging" aria-pressed="false">Messaging</button>
        <button class="tab-button" type="button" data-command-filter="context" aria-pressed="false">Context</button>
        <button class="tab-button" type="button" data-command-filter="jobs" aria-pressed="false">Jobs</button>
        <button class="tab-button" type="button" data-command-filter="mcp" aria-pressed="false">MCP</button>
      </div>
    </div>
  </div>
</section>

<section class="site-shell reference-layout">
  <div>
    <div class="reference-card">
      <strong data-result-count>0 commands</strong>
      <p>Rows remain visible without JavaScript; search and tabs progressively enhance the table.</p>
    </div>

    <div class="command-table-wrap" aria-label="Command reference table">
      <table class="command-table">
        <thead>
          <tr>
            <th scope="col">Command</th>
            <th scope="col">Use</th>
            <th scope="col">Key flags</th>
            <th scope="col">Output</th>
            <th scope="col">Recovery</th>
          </tr>
        </thead>
        <tbody>
          <tr tabindex="0" data-command-row data-category="messaging" data-command="handoff inbox --peek" data-use="Preview unread messages without consuming them." data-recovery="Run handoff inbox when ready to mark messages read.">
            <td><code>handoff inbox --peek</code></td>
            <td>Preview unread messages without marking them read.</td>
            <td><code>--session-id</code> <code>--all</code> <code>--limit</code> <code>--json</code></td>
            <td>Messages plus <code>marked_read</code> in JSON.</td>
            <td>Run <code>handoff inbox</code> when ready to consume.</td>
          </tr>
          <tr tabindex="0" data-command-row data-category="messaging" data-command="handoff to reviewer" data-use="Send a direct message or context handoff." data-recovery="Use handoff sessions to confirm the recipient exists.">
            <td><code>handoff to &lt;session|alias&gt; "message"</code></td>
            <td>Send a concise message to another live session.</td>
            <td><code>--subject</code> <code>--thread</code> <code>--context</code></td>
            <td>Message id, or JSON with <code>message_id</code>.</td>
            <td>Use <code>handoff sessions</code> for unknown recipients.</td>
          </tr>
          <tr tabindex="0" data-command-row data-category="messaging" data-command="handoff reply" data-use="Continue an existing thread." data-recovery="Use handoff history to locate the thread id.">
            <td><code>handoff reply &lt;thread-id&gt; "message"</code></td>
            <td>Reply inside an existing handoff thread.</td>
            <td><code>--as</code> <code>--stdin</code> <code>--file</code> <code>--subject</code></td>
            <td>Message id and thread id in JSON.</td>
            <td>Use <code>handoff history --json</code> to find the thread.</td>
          </tr>
          <tr tabindex="0" data-command-row data-category="context" data-command="handoff context create --git-diff" data-use="Capture a diff as reusable context." data-recovery="Provide exactly one or more context sources.">
            <td><code>handoff context create --git-diff</code></td>
            <td>Capture the current git diff as a context package.</td>
            <td><code>--title</code> <code>--as</code> <code>--json</code></td>
            <td>Context id, or JSON with <code>context_id</code>.</td>
            <td>Add <code>--text</code>, <code>--file</code>, <code>--stdin</code>, or <code>--cmd</code> when no input exists.</td>
          </tr>
          <tr tabindex="0" data-command-row data-category="context" data-command="handoff context show" data-use="Inspect context contents." data-recovery="Use handoff context list to find context ids.">
            <td><code>handoff context show &lt;context-id&gt;</code></td>
            <td>Inspect captured files, diffs, text, and command output.</td>
            <td><code>--json</code></td>
            <td>Header and ordered context items.</td>
            <td>Use <code>handoff context list</code> for recent packages.</td>
          </tr>
          <tr tabindex="0" data-command-row data-category="jobs" data-command="handoff run" data-use="Start background work for another agent." data-recovery="Check status and logs after queueing.">
            <td><code>handoff run &lt;agent&gt; --task &lt;text&gt;</code></td>
            <td>Queue a background task for a target agent.</td>
            <td><code>--context</code> <code>--git-diff</code> <code>--file</code> <code>--timeout</code> <code>--as</code></td>
            <td>Job id, or JSON with <code>job_id</code>.</td>
            <td>Run <code>handoff status &lt;job-id&gt;</code>.</td>
          </tr>
          <tr tabindex="0" data-command-row data-category="jobs" data-command="handoff delegate wait stdin git-diff" data-use="Delegate work and optionally wait for the result." data-recovery="Use status and logs with the returned job id if waiting fails.">
            <td><code>handoff delegate &lt;agent&gt; --task &lt;text&gt; --wait</code></td>
            <td>Create context, queue the job, wait for completion, and print the result.</td>
            <td><code>--stdin</code> <code>--context</code> <code>--git-diff</code> <code>--file</code> <code>--timeout</code></td>
            <td>Result body by default, or JSON with <code>job_id</code> and <code>result</code>.</td>
            <td>Run <code>handoff logs &lt;job-id&gt;</code> for adapter output.</td>
          </tr>
          <tr tabindex="0" data-command-row data-category="jobs" data-command="handoff status job" data-use="Read human job status." data-recovery="For blocked jobs configure adapter env vars and retry.">
            <td><code>handoff status &lt;job-id&gt;</code></td>
            <td>Show route, state, result, failure, and next action.</td>
            <td><code>--json</code></td>
            <td>Human summary by default, full job JSON with <code>--json</code>.</td>
            <td>For blocked jobs set <code>HANDOFF_AGENT_CMD_*</code> or <code>HANDOFF_RUNTIME_CMD_*</code>.</td>
          </tr>
          <tr tabindex="0" data-command-row data-category="jobs" data-command="handoff logs result retry" data-use="Inspect and recover job output." data-recovery="Retry failed, timed-out, or blocked work.">
            <td><code>handoff logs &lt;job-id&gt;</code></td>
            <td>Inspect adapter, stdout, and stderr logs; use follow mode to stream until completion.</td>
            <td><code>--tail</code> <code>--follow</code> <code>--json</code></td>
            <td>Log lines, or JSON log objects.</td>
            <td>Run <code>handoff retry &lt;job-id&gt;</code> after fixing the cause.</td>
          </tr>
          <tr tabindex="0" data-command-row data-category="mcp" data-command="HANDOFF_PROJECT" data-use="Pin MCP project resolution." data-recovery="Pass project at tool level when one server handles multiple repos.">
            <td><code>HANDOFF_PROJECT=/path/to/repo</code></td>
            <td>Pin MCP server cwd-sensitive identity resolution.</td>
            <td>Tool-level <code>project</code> overrides environment.</td>
            <td>MCP tools operate against the intended repo.</td>
            <td>Pass <code>project</code> per tool for multi-repo hosts.</td>
          </tr>
          <tr tabindex="0" data-command-row data-category="mcp" data-command="handoff_context_file handoff_reply handoff_delegate" data-use="Use expanded MCP tools for practical workflows." data-recovery="Fallback to CLI commands when host auth or tool configuration is incomplete.">
            <td><code>handoff_context_file</code></td>
            <td>Create context from a file or call <code>handoff_delegate</code> through MCP.</td>
            <td><code>file</code> <code>title</code> <code>asAgent</code> <code>project</code></td>
            <td>Text result containing CLI JSON plus matching MCP structured content.</td>
            <td>Use CLI <code>handoff context create --file</code> as fallback.</td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>

  <aside class="inspector-panel" aria-label="Selected command details">
    <div class="panel-head">
      <div>
        <h2>Inspector</h2>
        <p>Selected row details.</p>
      </div>
    </div>
    <dl>
      <div>
        <dt>Command</dt>
        <dd><code data-inspector-command>handoff inbox --peek</code></dd>
      </div>
      <div>
        <dt>Use</dt>
        <dd data-inspector-use>Preview unread messages without consuming them.</dd>
      </div>
      <div>
        <dt>Recovery</dt>
        <dd data-inspector-recovery>Run handoff inbox when ready to mark messages read.</dd>
      </div>
      <div>
        <dt>JSON output</dt>
        <dd><code>{"ok":true,"marked_read":false}</code></dd>
      </div>
    </dl>
  </aside>
</section>
