---
title: "What stops your coding agent from doing something it can't undo?"
seoTitle: 'What Stops a Coding Agent From Irreversible Actions?'
description: "Your coding agent's undo stops at local files. Learn how approval gates and scoped credentials help prevent irreversible actions in production."
pubDate: 2026-11-03
tags: ['agents', 'approvals', 'safety', 'claude-code', 'harness', 'observability']
ogImage: ../../assets/blog/what-stops-your-coding-agent-doing-something-it-cant-undo/og.png
faq:
  - q: 'Is auto mode safe enough to replace approvals?'
    a: 'For sensitive operations, no. Anthropic''s docs state explicitly that auto mode does not guarantee safety and is not a replacement for review on sensitive operations. Anthropic''s August 2026 figures show auto mode matching or beating manual review in their tests, but the March 2026 report recorded about 17% misses on 52 real overeager actions. Auto mode is a reasonable default for well-understood, lower-risk function classes. For production writes, deletions, or anything with external side effects, it shouldn''t be the only gate.'
  - q: 'Is a sandbox or container enough?'
    a: 'A sandbox limits what a mistake can reach, and a gate decides whether an action should happen at all. Pi''s docs point to a container or VM for isolation, and Cursor''s docs say explicitly that auto-review is not a security boundary. Use isolation and a gate together for external actions. The sandbox reduces blast radius if the gate fails, and the gate prevents the call from running at all.'
  - q: 'What should you do first after an agent deletes something?'
    a: 'Stop the agent and revoke the credential it used before you investigate anything else. An agent still running with the same credential can compound the damage while you''re reading logs. Check the platform''s soft-delete or backup window immediately. Then preserve the trace and transcript before retention clears them.'
---

![What stops your coding agent from doing something it can't undo? Reversible actions: let it run. Recoverable actions: allow and log. Irreversible actions: hold for a person.](../../assets/blog/what-stops-your-coding-agent-doing-something-it-cant-undo/banner.png)

A coding agent with access to real systems can delete a production database, send emails, or destroy cloud infrastructure before you finish reading the error message. The only reliable stop is a decision made before the call runs, whether a deny rule, a human hold, or a credential scoped narrowly enough that the operation can't succeed.

In 2026, [the founder of PocketOS watched Claude Opus 4.6](https://x.com/lifeofjer/status/2048103471019434248), running inside Cursor, delete the production database via Railway. The data came back only because Railway recovered it. The credential that made the deletion possible was intended to manage domains, not run destructive database operations, but it could do both.

Most developers pointing an agent at real systems carry a working assumption that checkpoints, `/undo`, and git mean any mistake is recoverable. That assumption breaks in a specific, dangerous way. File undo reverts edits to local files. With Git, you can revert the local codebase to any previous state. But databases, deploys, emails, payment records, and cloud state have no `/rewind` command or Git revert.

Two separate problems get collapsed into a single "approval workflow" story: stopping a call before it runs and knowing what an approved call did. The second problem is where most existing advice overpromises. This piece treats them separately, because conflating them produces systems where you think you're covered but aren't.

In this article, we'll cover what undo actually works for, why approving every call eventually fails, how to sort actions into three classes, where the gate belongs in your stack, and how to recover what gets through, including the limits of iii's own observability.

## What your agent's undo actually covers

File undo is a separate mechanism that restores edits made with the agent's own file tools and doesn't affect the systems those files communicate with.

The table below was built from each maker's current documentation, checked in October 2026.

| Tool | What undo restores | What it cannot reverse |
| --- | --- | --- |
| Claude Code (checkpoints, `/rewind`) | Edits made with Claude's file tools, plus the conversation | Bash edits, most subagent edits, external edits to files untouched in-session |
| OpenCode 2 (`/undo` snapshots) | Eligible files in the session's active directory of a Git repository | Commits, branches, databases, and processes |
| Cursor (checkpoints) | Agent's local file changes, stored apart from Git | Anything outside those files. The docs point to Git for the rest |
| Pi 1.0 (session tree) | The conversation, through `/tree` and `/fork` | Files, unless an extension adds checkpoints |
| iii (traces, logs) | Nothing. iii does not ship a file-undo or checkpoint feature | iii records what a turn did (traces, logs, the console's Waterfall and trace views) for a person to review, not to reverse |

## Why approving every call stops working

Attention decays faster than most teams expect. [Anthropic's own research](https://claude.com/blog/auto-mode-default-in-claude-code) found that paid testers caught a planted dangerous command roughly 17% of the time early in a session, dropping to about 5% after 50 or more prior prompts.

That's a predictable cognitive outcome, because approval volume trains people to click through. Anthropic's figures show [93% of prompts approved as of March 25, 2026](https://anthropic.com/engineering/claude-code-auto-mode), and [97% as of August 7, 2026](https://claude.com/blog/auto-mode-default-in-claude-code). A prompt approved 97% of the time has become a speed bump.

Researchers at Hugging Face and Data & Society put the structural problem plainly, showing that routine approval clicks push people out of the loop rather than keeping them in it.

A [report in IEEE Spectrum](https://spectrum.ieee.org/agentic-ai-humans-in-loop) from October 5, 2026 found that current human-review designs push people toward thoughtless clicking. It argues that meaningful oversight needs deliberate friction, such as requiring a reviewer to commit to a judgment before seeing the agent's plan.

Instructions in the conversation carry no enforcement weight, either. Replit's agent acted during a code freeze that had been stated in the conversation. In Claude Code's auto mode, a boundary stated in the conversation can be lost if compaction removes it. Also, [the docs note that auto mode does not guarantee safety and is not a replacement for review on sensitive operations](https://code.claude.com/docs/en/permission-modes).

The structural answer is fewer prompts requiring approval, applied to the right calls. Anthony Lusardi, our Principal Developer Advocate, describes their practice:

> "By default, nothing runs that we haven't explicitly allowed for that turn. The AI Agent Harness is deny-by-default, so a send with no `options.functions.allow` globs is just a chat loop. On top of that, approval-gate's default manual mode gates any call that doesn't match a configured rule. That means we usually don't write a rule for a new sensitive Function at all. Only explicit approvals run without review. What waits for a confirmation is everything else, which tends to be exactly the newest, least-reviewed, most sensitive functions."

The AI Agent Harness won't dispatch a Function unless it's explicitly allowed. The approval-gate Worker, running in its default manual mode, holds anything that matches no configured rule, which means the newest and most sensitive Functions wait by default rather than slipping through. Gate fewer, more consequential calls.

For a hands-on comparison of approval behavior across Claude Code, OpenCode, and iii, see [OpenCode vs Claude Code vs iii for Human in the Loop Workflows](https://iii.dev/blog/opencode-vs-claude-code-vs-iii/).

## How to sort actions by reversibility

Before an action runs, ask one question: if it goes wrong, can you reverse it without the affected platform's help? The answer maps to three classes.

| Action class | Examples | Default |
| --- | --- | --- |
| Reversible in the workspace | File edits, local tests, new branches | Let it run. Recover from the workspace if wrong |
| Recoverable at a cost | Push to a feature branch, staging deploy, dev database writes | Allow, log, review afterward |
| Irreversible or external | Production writes and deletes, `terraform destroy`, force push, email, payments, secrets | Hold for a person. Narrow credentials |

A few specific practices sharpen this framework.

- **Narrow credentials before the agent runs.** The PocketOS token could delete a production database because it was scoped far beyond domain management, making the credential's authority the real attack surface.
- **Prefer delayed deletes where the platform offers them.** Railway now soft-deletes API deletes for 48 hours after [their own database incident prompted a platform-level design change](https://blog.railway.com/p/your-ai-wants-to-nuke-your-database). Their rule is worth keeping: make the destructive thing slow and the recoverable thing fast.
- **Gate early.** Codex requires approval for any app or MCP tool flagged as destructive, and a read-only annotation on that same tool doesn't exempt it. The class of the action determines the gate, not the agent's stated intent.

## Where the gate belongs in your stack

Human-in-the-loop authorization for AI agents means a human decision is required before a tool with real-world authority can execute. The harder question isn't whether to gate; it's where in your stack that decision actually happens, and whether it can be bypassed.

A gate works in layers. The first check is usually coarse: is this action callable at all, for this session, right now? Only after that passes should a second, finer-grained check run, one that looks at the specific action, any rules configured for it, and whether a human has already approved something equivalent. Collapsing these into a single check tends to produce either too many prompts (everything falls to a human) or too few (narrow rules quietly broaden over time).

Holds should not expire. A pending decision that times out and defaults to either "proceed" or "silently fail" defeats the purpose of asking a human in the first place. A hold should wait exactly as long as it takes a person to look at it, survive process restarts, and leave a clear way to abort the action instead of letting it run unattended.

Who may approve matters as much as what requires approval. An agent, or a script acting on an agent's behalf, should not be able to resolve its own hold. That guarantee is stronger when it's enforced in more than one place: once at the point where the action is dispatched, and again at a layer the agent's own session can't reach or reconfigure, such as a network boundary or a separate authorization service it has no credentials for.

Placement also determines what the gate can't see. A gate wired into an agent's own tool-dispatch path only sees calls that go through that path.

Anything your application calls directly, outside the agent's loop, such as a bulk operation, an admin action, or a financial transaction triggered by your own backend, needs its own boundary.

Gating the agent's tool calls and gating your application's actions are two different problems. Solving the first can leave you assuming the second is covered when it isn't.

Finally, treat a resolved decision record as perishable unless you deliberately preserve it. Many gating tools keep a pending record only while a decision is outstanding, and reclaim it the moment it's resolved. If you need an audit trail, capture it explicitly; don't assume it persists on its own.

## How to recover what you let run

"Recovery" in this context means a person diagnosing what happened from the evidence available and fixing it by hand, not an automated revert step. Most agent platforms, including iii, don't ship one. Good observability narrows the diagnosis; it doesn't substitute for the fix.

Consider a representative scenario: an agent working on a feature branch opens a pull request, runs a database migration on staging, updates application state across a few services, and then, because no rule had been written for the delete action yet, drops a production table before any hold has a chance to fire.

Afterward, a complete trace lets a person reconstruct exactly what happened instead of guessing from symptoms. It shows every call in the turn, in order, with its arguments and result.

That means the trace needs to actually be there when you need it. Two defaults work against you here: trace storage usually has an age limit and a size cap that evicts the oldest data first, and logs or metrics are often kept in memory only, cleared within an hour or so. If you need to go back further than that, export to a durable, queryable store before the defaults clear the data; don't assume the platform is keeping everything indefinitely.

Being able to retry from an earlier point in a conversation is a different capability from reverting what already happened.

A fork or branch of a session lets you resume from before a decision was made, but it has no effect on anything the agent already did outside the conversation itself. Files, databases, and any other state stay exactly as the original run left them.

Separate the tasks a person must do from the evidence the system can provide. Resolving a hold, deciding whether to retry a denied call, revoking a compromised credential, and restoring from the affected platform's own backup or soft-delete window stay with a person; no agent platform does these for you.

What a good trace gives you is the evidence: exactly what ran, in what order, with what result. The investigation starts there. The correction happens in the platform that actually owns the data.

## Where iii fits

iii is a framework for building intelligent systems in which agents call the same Functions as the application, so both show up in the same traces and have identical behaviors.

A billing service call and an agent's `terraform::apply` appear in the same Waterfall view, with the same span structure.

The approval-gate Worker works in layers: The AI Agent Harness checks the function allow/deny policy first. A Function not in `options.functions.allow` never reaches the gate at all, returning `policy_denied` immediately.

Only after that policy check passes does the approval-gate `pre_trigger` hook run, evaluating per-session mode, allow-lists, and inline config rules, and answering continue, deny, or hold. In manual mode, the default, a remembered approval still allows the call; otherwise, rules determine the outcome, and unmatched calls are held.

Holds don't expire. A held call waits until a human resolves it, the turn aborts, or the session is deleted.

The approval-gate Worker keeps no resolved-approval history; a record exists only while a call is held. Once a hold resolves, the transcript's `function_result` and the `pending_resolved` event are the remaining evidence. Export or screenshot them before retention clears them. Traces default to 30-day retention (`trace_storage.retention_seconds`, default 2,592,000 seconds) with a 1 GiB archive cap that evicts the oldest data first. Logs and metrics are kept only in memory, with a 1-hour default window. `engine::traces::clear` and `engine::logs::clear` delete data and don't let you go back to it. For long-term storage, the iii team's recommendation is to export to an OTel-compatible service before the defaults clear it.

Agents are prevented from resolving their own holds. approval-gate's full mode hardcodes a deny on `approval::*`/`configuration::*` no matter what else is configured, and for the default manual/auto modes, the guidance is to keep those same functions out of whatever allow-list the harness gives an agent in the first place.

rbac-proxy is a standalone Worker that sits between the network and the engine, evaluating an ordered decision (forbidden > scoped grant > allowed > infrastructure carve-out > `expose_functions` > deny) on every incoming invocation. It gates which Functions a session may invoke at all, including which Workers an agent or user can even see in discovery. When used alongside approval-gate, rbac-proxy fires first.

That layering has a boundary. approval-gate is a `pre_trigger` hook on harness dispatch, so it doesn't intercept a call that never goes through that path. A direct call to a function can bypass approval-gate entirely. Application-level gating for those direct calls runs through rbac-proxy at the network edge, not inside the turn loop. Those calls include bulk operations, admin actions, and anything your backend triggers outside the agent's loop.

Recovery works the same way, by design. Anthony describes the approach:

> "iii doesn't have a revert step just yet and, depending on the system, might not be possible due to outside state changes. Recovery is straightforward, however, for both agents and users: look at the trace, diagnose what went wrong, write tests, and fix. iii's inherent observability makes it effective to 'roll back' without needing to store snapshots."

`engine::traces::list` returns every span across a turn, and `engine::traces::tree` shows the span tree for one trace, including which Function called which other Function, in what order, with what arguments and results; the console's Waterfall view makes that sequence readable without querying the API directly.

Start with the harness via the [iii install guide](https://iii.dev/docs/install), then run `iii call compose::add worker=approval-gate` to get started with the approval-gate.

## What else to know before an agent touches production

### Is auto mode safe enough to replace approvals?

For sensitive operations, no. [Anthropic's docs state explicitly that auto mode does not guarantee safety and is not a replacement for review on sensitive operations](https://code.claude.com/docs/en/permission-modes). Anthropic's August 2026 figures show auto mode matching or beating manual review in their tests, but the March 2026 report recorded about 17% misses on 52 real overeager actions. Auto mode is a reasonable default for well-understood, lower-risk function classes. For production writes, deletions, or anything with external side effects, it shouldn't be the only gate.

### Is a sandbox or container enough?

A sandbox limits what a mistake can reach, and a gate decides whether an action should happen at all. Pi's docs point to a container or VM for isolation, and [Cursor's docs say explicitly that auto-review is not a security boundary](https://cursor.com/docs/agent/security/run-modes). Use isolation and a gate together for external actions. The sandbox reduces blast radius if the gate fails, and the gate prevents the call from running at all.

### What should you do first after an agent deletes something?

Stop the agent and revoke the credential it used before you investigate anything else. An agent still running with the same credential can compound the damage while you're reading logs. Check the platform's soft-delete or backup window immediately. Then preserve the trace and transcript before retention clears them.
