---
title: 'OpenCode vs Claude Code vs iii for human in the loop workflows'
seoTitle: 'OpenCode vs Claude Code vs iii: Hands-on Approval Tests'
description: 'We tested the same sensitive tool calls in OpenCode, Claude Code, and iii. See how approvals work and the strengths of each option.'
pubDate: 2026-10-19
tags: ['agents', 'comparison', 'claude-code', 'opencode', 'approvals', 'harness']
ogImage: ../../assets/blog/opencode-vs-claude-code-vs-iii/banner.png
faq:
  - q: 'Will OpenCode 1 plugins run in OpenCode 2?'
    a: 'No. OpenCode 2 has a new plugin API, and v1 plugin code does not run in v2. Most v1 config, agents, commands, and skills keep working, and both versions install under the same opencode command, so check which version is active before assuming v2 features are available.'
  - q: 'What does each option cost before inference?'
    a: 'OpenCode and iii are free to run; you pay only for inference. Claude Code requires a paid Claude plan (Pro $20/month, Max from $100/month, Team Standard $25/seat/month as of 2026-10-07) or API billing at usage-based token pricing. OpenCode Go is $10/month with metered usage caps.'
  - q: 'Is iii open source?'
    a: 'The iii engine and CLI use the Elastic License 2.0, which is not OSI-approved. You can use and self-host the engine; the only limitation is that you cannot offer it to others as a hosted or managed service. The SDKs, console, and published workers use Apache 2.0.'
  - q: 'Do these tools send my code to their servers?'
    a: 'All three send prompts and code context to whichever provider runs the model. With OpenCode or iii that can be a local model, so no context leaves your machine at inference time. Claude Code sends to Anthropic. iii is self-hosted and sends only anonymous AI Agent Harness usage events with no message content unless III_TELEMETRY_ENABLED is set to false.'
  - q: 'Which tool survives a restart with a pending approval?'
    a: 'In our test only iii did. A held call in iii became a pending record in shared engine state and survived both an AI Agent Harness restart and a full engine restart. OpenCode lost a pending approval on a service restart, and killing Claude Code abandoned the stuck call with no re-prompt.'
---

![OpenCode vs Claude Code vs iii: how do they handle approvals? Claude Code handles single-user approvals. OpenCode handles single-user and multi-user approvals. iii handles single-user, multi-user, and cross-team, agent, and organization approvals.](../../assets/blog/opencode-vs-claude-code-vs-iii/banner.png)

Your agent edits files and runs tests without any help. It can even call sensitive functions with potentially destructive results (for example, deleting files or changing production data).

In an agent, the destructive functions can be gated with an approval prompt, but how do the tools handle approval, denial, or ignoring a sensitive tool call?

The top-ranking comparisons of OpenCode vs Claude Code usually stop at models, price, speed, and terminal UX, and none of them test what happens when you approve, deny, or ignore a sensitive tool call.

That's what we test in this article. We created a single `set_flag` action that writes one key to `config/flags.production.json` and logs to `audit/flags.log`, and the write must wait for a person. For OpenCode and Claude Code, the tool ran as a stdio MCP server.

For iii, it ran as a worker Function (`release::set_flag`). For OpenCode, we also built a real OpenCode v2 native plugin to confirm a documented behavior delta. None of the 3 writes does its own permission checking, so every result reflects the host's enforcement.

> **Versions tested, checked 2026-10-07.** OpenCode v2.0.24, Claude Code v2.1.292, iii engine 0.24.4 / AI Agent Harness 1.8.44 / approval-gate 1.0.22. All three used claude-sonnet-5 with the same Anthropic API key.

## Quick verdict by workload

OpenCode and Claude Code are terminal agents built for code editing and PR workflows.

Our framework lets agents and application code share registered Functions. In this test, approval-gate held calls dispatched through the AI Agent Harness; direct application calls needed a separate authorization boundary.

Which tool fits depends on who else needs to see a pending approval: nobody but you (Claude Code), your own team's other sessions (OpenCode), or any system that can reach your backend (iii).

So, reach for Claude Code or OpenCode to edit code and open PRs. Reach for iii when building complex, end-to-end intelligent systems where a sensitive call needs to wait for a person using a shared state that any client can reach.

<div class="verdict">
  <div class="verdict-card">
    <p class="verdict-pick">Best for staying on Claude models with managed approvals</p>
    <p class="verdict-tool">Claude Code</p>
    <p class="verdict-reason">An explicit ask rule still overrides auto mode, confirmed in our run: "Ask rule mcp__release__set_flag overrides auto mode for this tool".</p>
    <a class="verdict-cta" href="https://iii.dev/docs/install">Claude Code in iii</a>
  </div>
  <div class="verdict-card">
    <p class="verdict-pick">Best for approvals shared across your own local clients</p>
    <p class="verdict-tool">OpenCode</p>
    <p class="verdict-reason">A pending approval raised by one client is visible to and answerable from any other client attached to the same background service. Only through its MCP route, though: the v2 plugin route skips the ask check entirely, confirmed by a write landing with no prompt at all.</p>
    <a class="verdict-cta" href="https://iii.dev/docs/install">OpenCode in iii</a>
  </div>
  <div class="verdict-card" data-highlight>
    <p class="verdict-pick">Best when the approval gate must run in your backend</p>
    <p class="verdict-tool">iii</p>
    <p class="verdict-reason">A held call became a pending record in shared state. It survived both an AI Agent Harness restart and a full engine restart in our run, which neither other tool's pending state did.</p>
    <a class="verdict-cta" href="https://iii.dev/docs/install">Start the harness</a>
  </div>
</div>

## How we ran the test

We tested 5 decisions per tool: approve once, approve and remember, deny with a reason, and leave pending. For the pending case, we checked the state after a client reconnected, after a service restart, and again after 35 minutes.

<div class="compare">

| Setup | OpenCode | Claude Code | iii |
| --- | --- | --- | --- |
| Version tested | 2.0.24 | 2.1.292 | Engine 0.24.4, AI Agent Harness 1.8.44, approval-gate 1.0.22 |
| Model and billing | claude-sonnet-5, API key | Same | Same |
| Where the tool runs | MCP server | MCP server | iii worker Function |
| Where the rule is | `opencode.jsonc` | `.claude/settings.json` | approval-gate's default manual mode (no rule needed) |
| Starting permission mode | No default mode, rules apply directly | Auto mode (confirmed, v2.1.283+) | Deny-by-default for custom Functions, callable per-send via `options.functions.allow` |

</div>

## Two terminal agents and one framework you assemble

All 3 can gate a tool call before it runs. The difference is which process owns the check, which tool paths it covers, and who may answer it.

**At a glance:** OpenCode's MCP route and Claude Code both check the call inside the process running the agent, before dispatch, and show the result in that same client.

iii's AI Agent Harness checks it before the Function dispatches and writes the hold into state shared across the engine, which is why a separate client (a second browser tab, or the console after a restart) can see and resolve it. OpenCode's plugin route skips the check entirely for ask and acts only on deny.

### OpenCode

OpenCode is an open-source coding agent from Anomaly (MIT license) with terminal, desktop, and web clients.

Among the 2 terminal agents here, it offers the widest model access, with 75+ providers and a v2 plugin API. For custom tools that need a gated write, the MCP route is the one that actually works.

#### Key facts

<div class="facts">

- **Version tested:** v2.0.24 (v2 track, with v1 still shipping in parallel as 1.18.x, and v1.18.35 shipped the same day)
- **License:** MIT
- **Model access:** 75+ providers via API key, including Claude, DeepSeek, and local models
- **Where custom tools come from:** v2 plugins or MCP servers
- **Where the approval decision is recorded:** a local SQLite database at `~/.local/share/opencode/opencode.db`, permission table

</div>

#### Why it stands out

OpenCode's background service architecture means multiple clients can connect to the same session, and an approved "Allow always" rule takes effect project-wide immediately.

Rules are an ordered list of allow, ask, and deny entries where the last match wins. The base policy allows most operations except external directory access and `.env` reads, which trigger ask, so a new MCP tool runs without prompting until you add a rule in `opencode.jsonc`.

#### Limitation

The v2 [plugin](https://opencode.ai/v2/docs/plugins) route does not pause on ask at all, confirmed by the write landing unprompted, and only hides the tool from the model's catalog on an explicit deny.

We confirmed this by building a real plugin and testing both halves. An ask rule on the plugin's action had no effect (the call ran unprompted), and an explicit deny rule removed the tool from the model's catalog.

The MCP route (`codemode: false`) is the one that produces a real "Permission required" card with Deny, Allow once, and Allow always. One additional headless behavior to know about is that `opencode run` from a script auto-rejects an ask rule ("permission requested... auto-rejecting") because no interactive client is attached. A genuinely pending call needs a live client, specifically the browser pairing flow (`opencode pair`) attached to the background service.

Also relevant for team choices, OpenCode 1 plugins do not run in OpenCode 2. The v2 plugin API breaks from v1. Most v1 config, agents, commands, and skills keep working, but plugin code needs to be rewritten.

Both versions install as the `opencode` command, so check which version is active before assuming v2 features are available.

#### Best for

Teams that want to pick their own models and own the extension code. For the gated write tested here, use the MCP route with an explicit ask rule.

### Claude Code

Claude Code is Anthropic's agentic coding tool for terminal, IDE, desktop, and browser.

Its approval controls are the best-documented of the 3, with layered rules, permission modes, MCP approval flags, and hooks all designed for one developer's session. It is proprietary, with use governed by Anthropic's commercial or consumer terms.

#### Key facts

<div class="facts">

- **Version tested:** v2.1.292
- **License:** proprietary (Anthropic terms)
- **Model access:** Claude only, officially
- **Where custom tools come from:** MCP servers, often packaged in plugins
- **Where the approval decision is recorded:** `.claude/settings.local.json` (durable, per-repository) for persisted rules, with file-modification approvals lasting only until session end

</div>

#### Why it stands out

Auto mode (the default from v2.1.283+ for interactive terminal/VS Code sessions) classifies tool calls and decides whether to proceed without prompting. An explicit ask rule overrides auto mode, confirmed in our run.

Setting `_meta["anthropic/requiresUserInteraction"]` (available from v2.1.199+) removes the "don't ask again" option entirely, and the prompt drops from 3 options to 2 ("Yes" / "No" only, no persistent-approval path). Rule edits in `/permissions` apply from the next tool call with no restart required.

#### Limitation

Approvals are answered inside a live session. An unanswered interactive prompt just blocks indefinitely, and killing and resuming the process abandons the stuck call with no re-prompt.

The one built-in way to queue a call for later is the defer PreToolUse [hook](https://code.claude.com/docs/en/hooks). We confirmed it works in headless `-p` mode, where the terminal output showed `"terminal_reason":"tool_deferred"` and nothing executed, but reliably delivering a new decision on resume was flaky in our test (changing the hook to allow and resuming came back denied), consistent with an open upstream report.

Treat defer as a working pause, not a dependable resume. Session files are deleted after 30 days by default (`cleanupPeriodDays`), so the chat transcript's usefulness as an approval audit log has a 30-day ceiling, although you can configure it to have a higher value.

Current US plan prices, checked 2026-10-07: Pro $20/month or $200/year (advertised as $17/month billed annually); Max starts at $100/month (5x) or $200/month (20x); Team Standard is $25/seat/month monthly or $20/seat/month billed annually with a minimum of two seats. Taxes extra. Claude Code is not included in the Free plan.

#### Best for

Developers already paying for Claude who want documented approval controls without writing enforcement code. Auto mode decides on the user's behalf.

### iii

> **We build iii, so this comparison is written from our perspective.** The tests below use the same write operation and report each tool's limitations, including ours.

iii is a framework for building intelligent systems where an agent's tools and your application's tools are registered on the same engine.

The AI Agent Harness is a thin, durable turn loop whose tools are Functions.

Compared to the two terminal agents here, iii can handle approval gates for any connected client.

#### Key facts

<div class="facts" data-highlight>

- **Version tested:** engine 0.24.4, AI Agent Harness 1.8.44, approval-gate 1.0.22
- **License:** engine and CLI under Elastic License 2.0 (not OSI-approved); SDKs, console, and published workers under Apache 2.0
- **Model access:** any provider via provider workers (not an inference provider itself)
- **Where custom tools come from:** Functions a worker registers with the Node, Browser, Python, Rust, or Go SDK
- **Where the approval decision is recorded:** a record in shared engine state, visible via the console, `approval::list-pending`, or any other connected client

</div>

#### Why it stands out

Approval-gate's manual mode holds any call matching no configured rule. We wrote no explicit rule for `release::set_flag` at all, and its absence was sufficient to gate it.

The standout result of the whole test was that the held call survived both an AI Agent Harness-only restart (`compose::restart worker=harness`) and a full engine process kill-and-restart, with the exact same pending call intact afterward in both cases.

The other two tools' pending state did not survive restart. But in iii, our pending call was visible to and answerable from a second client attached to the same engine. OpenCode also supports multiple clients through its background service; the distinction in this test was that iii's pending state survived a full engine restart. This means you can loop a human into the approval process without having to worry about keeping the iii session running.

#### Limitation

Getting to a gated workflow meant adding two workers (release-flag and approval-gate) to a compose file.

No native Windows support (WSL2 only). The harness also makes a separate, gated `engine::functions::info` lookup before the real call, resulting in two sequential approval prompts that added friction to this specific strict enforcement test. This could be avoided by adding `engine::functions::info` to the approval gate.

#### Best for

Teams that need strict approval gating across agents and teams, or that need approval gates from development through production. Follow [our install guide](https://iii.dev/docs/install) to start the AI Agent Harness, then [add the approval-gate worker](https://github.com/iii-hq/workers/blob/main/approval-gate/README.md) for harness-dispatched calls. Configure a separate access boundary for direct application calls.

## What happened when we gated the same tool 3 ways

### Where the approval is enforced

OpenCode's MCP route produces a native "Permission required" card (Deny / Allow always / Allow once) inside the client attached to the background service.

<figure>
  <img src="../../assets/blog/opencode-vs-claude-code-vs-iii/opencode-permission-required.png" alt="OpenCode's browser client showing a pending Permission required card for release_set_flag, with Deny, Allow always, and Allow once buttons." width="2048" height="1007" />
  <figcaption>OpenCode's pending approval card in the browser client: "Permission required" with Deny, Allow once, and Allow always.</figcaption>
</figure>

Its v2 plugin route, tested separately by building a real plugin, does not pause on ask at all, confirmed by the write landing unprompted, and only hides the tool from the model's catalog on an explicit deny.

The MCP route is what this article presents as OpenCode's working mechanism, and the plugin route is a confirmed limitation.

Claude Code's status line named the override directly, reading "Ask rule mcp__release__set_flag overrides auto mode for this tool." Setting `_meta["anthropic/requiresUserInteraction"]` dropped the prompt to Yes/No only, removing the persistent-approval option entirely.

iii's approval-gate held the call by default, confirmed via approval-gate hook bindings confirmed in the engine log. In our test, the pending approval was shared across clients attached to the same engine.

A pending call raised in one browser tab was visible to, and answerable from, a second browser tab attached to the same engine, with no state transfer required.

<figure data-highlight>
  <img src="../../assets/blog/opencode-vs-claude-code-vs-iii/iii-pending-approval-console.png" alt="The iii console with the chat and traces panes side by side: release::set_flag is paused with approve, deny, and approve-always buttons, and the traces pane shows the held harness::send call with its spans across the harness, approval-gate, and provider workers." width="2048" height="1056" />
  <figcaption>The same pending call in the iii console, seen from a second connected client: the held record with Approve and Deny, and the full trace of the harness dispatch beside it.</figcaption>
</figure>

Who else can answer a pending call follows from where the hold lives. OpenCode's background-service clients can all see a pending card because they share the service, so the hold is local to that service.

Claude Code's hooks cannot override an explicit ask rule, and an unanswered prompt blocks the session rather than becoming a shared record. iii explicitly denies agents the `approval::*` Functions on the gate path as a hardcoded self-escalation defense (the agent cannot approve itself), and its rbac-proxy Worker exists to namespace who may approve or deny what across teams and roles.

The process that owns the check decides who can see and answer a pending approval, whether that's a single session (Claude Code), a shared local service (OpenCode), or shared global system-wide state any client can reach (iii).

### Approve, deny, and leave it pending

All 3 execute an approved call identically, with the file and audit log both writing and matching the shared logic exactly.

**"Approve and remember" diverges sharply.** OpenCode's "Allow always" writes a project-wide wildcard rule that takes effect immediately and retroactively.

A call left pending in a separate session got approved the moment we granted "Allow always" in a different session of the same project, confirmed by reading the saved rule directly out of OpenCode's SQLite database.

Claude Code's "don't ask again" cannot override an explicit ask rule at all, so granting it and immediately re-triggering the same call prompted again.

iii's "approve always" is scoped to the rest of the current conversation only, and says so up front in a confirmation dialog ("you're about to let the agent trigger release::set_flag for the rest of this conversation... this function matched the potentially-destructive verb policy"). A fresh session went straight back to asking.

**Denial is where the three diverge most** in what the model is told. OpenCode shows a bare "Interrupted" with no text, so the model receives nothing explaining why.

Claude Code depends on whether a reason was typed. No reason stops the turn outright, and a typed reason is delivered to the model, which in our run continued working and wrote itself a persistent memory note about the denial reason, unprompted.

iii returns a structured JSON error as an ordinary Function result, and the model explained the rejection in plain language without stopping:

```json
{
  "error": {
    "message": "Rejected by operator.",
    "details": { "denied_by": "user", "status": "denied" }
  }
}
```

The web console's "deny" button uses a fixed "Rejected by operator." message with no free-text field, though the underlying `approval::resolve` Function does accept a custom reason parameter, confirmed via direct CLI testing, so a custom denial reason is available at the API level, just not from that particular button.

**Pending state and restarts** are where the 3 diverge most operationally.

<div class="compare">

| Decision | OpenCode | Claude Code | iii |
| --- | --- | --- | --- |
| Approve once | Executes, writes file and audit log | Executes, writes file and audit log | Executes, writes file and audit log |
| Approve and remember | Project-wide, immediate, retroactive to other pending calls in the same project | Does not override an explicit ask rule | Session-scoped only, with an explicit "potentially destructive" confirmation dialog before granting |
| Deny with a reason | Turn interrupts, no text shown to the model | No reason: turn stops outright. With a typed reason (Tab to amend): model sees it and continues | Structured JSON error delivered to the model as a Function result, model continues and explains the rejection |
| Pending, then restart | Lost on a service restart. A plain client reconnect without restart does not lose it | Killing the process abandons the stuck call with no record and no re-prompt. Headless `-p` mode's defer hook survives the process exiting, but resuming with a new decision was unreliable in our test | Survived both an AI Agent Harness-only restart and a full engine process kill-and-restart, with the exact same card intact |
| Pending after 35 minutes | Untouched, no timeout; resolved normally after the wait (session elapsed-time counter read 38m 47s) | Untouched, no timeout; the interactive prompt just blocks indefinitely | Untouched server-side, confirmed via `approval::list-pending` (the browser view itself went stale after idle, a client-side quirk and not a state loss); no timeout |
| Left for an audit | No dedicated timestamped log of individual decisions present, the chat transcript is the record | Chat transcript is the record; `--resume` replays it, including the abandoned stuck call with no annotation | approval-gate, like all iii functionality, is end to end traceable. Default observability retention is 30 days and 1GB of data, user configurable, and exportable to any OTel-compatible provider |

</div>

### What you maintain after the demo works

**OpenCode's maintenance surface** is a plugin or MCP server plus permission rules in `opencode.jsonc`. Both reload live, with no restart needed to iterate. The permission table in the local SQLite database accumulates "Allow always" rules as you test, so scrub it periodically if you want clean test runs.

**Claude Code's surface** is an MCP server plus settings files. Rule edits in `/permissions` apply from the next tool call, and managed settings can push rules to a team. The defer hook decision shape is newer and less battle-tested than the core permission system. Session files age out after 30 days by default, so the chat transcript has a 30-day ceiling as an approval audit log.

**iii's surface** is a Worker, one `worker-compose.yaml` entry, and approval-gate's own configuration, which hot-reloads.

**The main conclusion on scope.** The approval lives in one developer's agent in the smallest setup, and in a shared AI Agent Harness that several agents and people use in the widest one.

Keep in mind that the AI Agent Harness only gates the harness dispatch path.

A direct `iii trigger release::set_flag` call bypassed approval-gate entirely in our test, because that's a direct Trigger rather than a harness dispatch. iii's rbac-proxy Worker exists to put a role/team boundary in front of direct calls like that one.

<div class="compare">

| Maintenance | OpenCode | Claude Code | iii |
| --- | --- | --- | --- |
| Files you add or change | `opencode.jsonc`, optionally `.opencode/plugins/<id>/index.ts` | `.mcp.json`, `.claude/settings.json` | `worker-compose.yaml` entry, one worker directory |
| APIs you depend on | v2 plugin API or MCP | MCP, settings, hooks | iii SDK, approval-gate Functions |
| Upgrade exposure | v1 plugins do not run on v2; MCP config format stable across tested versions | Settings/MCP config format stable v2.1.199 to v2.1.292; defer hook decision shape is newer and less battle-tested | approval-gate API is stable; updates via `compose::add` |
| Sharing with a teammate or second agent | Project-wide allow rules in a local SQLite database, with no built-in role separation. Can be shared with another agent on the same computer | Managed settings can push rules org-wide, and approval still lives in one session at a time | rbac-proxy Worker adds namespaced authorization by role, team, or concern across agents and humans |

</div>

## Does an approval prompt make the write safe?

Approval prompts reduce risk but don't eliminate it. Anthropic's own data shows the challenge clearly.

In Anthropic's telemetry as of March 25, 2026, [roughly 93% of Claude Code's permission prompts were approved](https://www.anthropic.com/engineering/how-we-contain-claude).

By the time Anthropic announced auto mode as the Pro/Max/Team default on August 7, 2026, that figure had risen to [97% approved, 3% rejected](https://claude.com/resources/articles/auto-mode-default-in-claude-code). In a separate study of 1,053 paid testers, manual reviewers caught a planted dangerous command 13.6% of the time (143/1,053); Anthropic's classifier caught 89% (937/1,053).

Human catch rates fell from about 17% early in a session to about 5% after 50 or more prior prompts, while the classifier's rate held flat. Anthropic's own advice is to review high-stakes production changes yourself rather than relying on a prompt you've already clicked past dozens of times that session.

Approval decides whether one write happens, but it doesn't constrain how much damage a mistake can do if it gets through. [OpenCode's own docs warn that the shell tool runs with the host user's full authority](https://opencode.ai/v2/docs/permissions). Our docs distinguish authorization from isolation: RBAC controls who may call what, but it is not a sandbox, although VM-level isolation is planned.

## How iii wraps the other two tools

You can try iii without replacing your agent. The opencode and claude-code workers can run each CLI as a Function (`opencode::run`, `claude::run`) and stream its events into the iii console. iii also offers Claude and OpenCode Go as providers to use directly in its harness interface.

For Claude Code, the worker's optional `approval_gate` setting routes each call needing approval through iii's own `policy::check_permissions` check. That check is synchronous and inline. If the policy worker is unreachable, the gate fails closed. Calls that Claude Code already allows skip the check entirely.

[Our OpenCode worker](https://github.com/iii-hq/workers/blob/main/opencode/README.md) leaves local file edits to OpenCode's native tools. If you dispatch `opencode::run` through our gated AI Agent Harness, approval applies to that outer Function call, not to each edit inside it.

We provide the harness and worker integrations, not model inference. This approval test did not measure speed, token savings, or integration costs, so it should not be read as evidence of those benefits.

Follow [our install guide](https://iii.dev/docs/install), then configure the AI Agent Harness with [approval-gate](https://github.com/iii-hq/workers/blob/main/approval-gate/README.md) to get started.

## What else to know before you choose

### Will OpenCode 1 plugins run in OpenCode 2?

OpenCode 2 has a new plugin API, and v1 plugin code does not run in v2. Most v1 config, agents, commands, and skills keep working across the version line, and both versions install under the same `opencode` command, so check which version is active before assuming v2 features are available. The migration guide covers what transfers and what you need to rewrite.

### What does each option cost before inference?

OpenCode and iii are free to run, and you pay only for inference. Claude Code requires a paid Claude plan. Current prices, checked 2026-10-07, are Pro $20/month ($17/month billed annually), Max from $100/month (5x) or $200/month (20x), and Team Standard $25/seat/month ($20/seat billed annually). However, Claude Code can also be run via API billing using usage-based token pricing instead of a flat monthly subscription.

OpenCode Go is $10/month, single tier (higher tiers were removed in August 2026), with metered usage caps ($12/5hr, $30/week, $60/month, with lower sub-caps on premium models).

### Is iii open source?

The engine and CLI use the Elastic License 2.0, which is not on the OSI list of approved licenses. ELv2 lets you use and self-host the engine, the only limitation is that you can't offer it to others as a hosted or managed service. The SDKs, console, and published workers use Apache 2.0.

### Do these tools send my code to their servers?

All three send prompts and code context to whichever provider runs the model. With OpenCode or iii, that can be a local model, so no context leaves your machine at inference time. Claude Code sends to Anthropic, so see [Anthropic's data-usage page](https://www.anthropic.com/legal/privacy) for training and retention rules by plan.

OpenCode's homepage states it does not store your code or context, though hosted Go and Zen models have per-model retention rules, and OpenCode's `/share` command, when used, explicitly syncs the full conversation to OpenCode's servers for anyone with the link, unless disabled via `share: "disabled"` in `opencode.json`.

iii is self-hosted. The engine sends anonymous AI Agent Harness usage events with no message content unless `III_TELEMETRY_ENABLED` is set to false.

## Which tool fits your workflow

The right choice follows from where you need the approval to live. Claude Code is the fastest path to a working gated call if you're already on a paid Claude plan, requiring two config files, no restart, and an explicit ask rule that overrides auto mode.

OpenCode gives you model choice and an MIT license, with the MCP route as the mechanism that actually gates writes.

Consider our AI Agent Harness when you need to enforce strict agent behavior, when enforcement is needed across development through to production, and in multi-agent and multi-user situations. If application code also calls the same backend Function directly, configure authorization for that via rbac-proxy; approval-gate is for harnesses.

All 3 reduce risk. Anthropic's own data puts human catch rates at roughly 5% late in a session, which means the gate is only as good as the attention behind it.

Pair whichever tool you choose with a boundary behind the gate, whether that's iii's rbac-proxy Worker, a scoped IAM role, or a write-protected deployment pipeline, so a missed approval doesn't become an unrecoverable production change.

Follow [our install guide](https://iii.dev/docs/install), then configure the AI Agent Harness with [approval-gate](https://github.com/iii-hq/workers/blob/main/approval-gate/README.md). Test a harmless Function call through approve, deny, and pending before connecting production actions.
