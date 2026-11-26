---
title: '9 best agent harnesses for developers who want more control'
seoTitle: '9 Best Agent Harnesses for Developers Who Want Control'
description: 'Compare 9 agent harnesses on model choice, custom tools, approvals, and sandboxing. See the defaults and trade-offs before choosing one for your workflow.'
pubDate: 2026-12-02
tags: ['agents', 'harness', 'comparison', 'approvals', 'sandboxing']
ogImage: ../../assets/blog/best-agent-harnesses/og.png
faq:
  - q: 'What is the difference between an agent harness and an agent framework?'
    a: 'An AI Agent Harness supplies a runnable loop, the execution structure that calls the model, routes tool calls, and manages turns. A framework supplies building blocks you assemble into a loop yourself. The categories overlap: some projects market themselves as frameworks but ship a runnable harness, and some harnesses expose enough extension points that you can rebuild their internals. For approvals and isolation, the defaults of a given harness are your starting point, while a framework''s defaults only come into play once you have wired everything together.'
  - q: 'Which coding agent has the best harness?'
    a: 'It depends on the model and the task. Composio''s single-model business-workflow test found a 20-percentage-point spread across harnesses, which is large enough to change which one you would pick. That test does not tell you how the harnesses compare on your codebase with your preferred model and approval configuration. Test your top 2 or 3 candidates on a representative sample of your own tasks before committing to one.'
  - q: 'Can I run these harnesses with a local model?'
    a: 'The 8 harnesses other than the Claude Agent SDK document local or self-hosted provider paths. iii connects through its llm-router worker with a llama.cpp provider. Pi accepts local GGUF files. OpenCode supports local models through its provider configuration. Codex CLI uses codex --oss with a compatible Ollama or LM Studio endpoint. goose routes through its provider system, which includes local options. Hermes Agent supports Ollama and vLLM. LangChain Deep Agents accepts any compatible LangChain tool-calling model, including locally hosted ones. OpenHands uses LiteLLM, which proxies local endpoints. The Claude Agent SDK is documented for Claude-backed providers rather than arbitrary local inference.'
---

![9 agent harnesses compared on control: a table of harnesses with one row per harness and a cell per control criterion.](../../assets/blog/best-agent-harnesses/banner.png)

You've been running a hosted coding agent for a few months. It works well until you need to wire in a tool that calls your internal API, swap the model for one you can run locally, or stop an action before it writes to production.

A hosted coding agent typically limits your control over how it works. At that point, the hosted product stops being useful, and you start looking for something you can actually configure.

An AI Agent Harness is the software around a model that runs the loop, tools, context, and permissions. The harness controls most decisions: which model gets called, which tools require human approval, what process isolation exists, and where everything runs.

[Composio ran a vendor-side test](https://composio.dev/content/best-ai-agent-harnesses) across 8 harnesses using 1 model on 25 business-workflow tasks and found pass rates between 68% and 88%, a 20-percentage-point spread. It tells you that the execution layer matters as much as the model.

Our AI Agent Harness is built from separate workers covering the turn loop, sessions, context, model routing, approvals, and sandboxes. Each one is replaceable without touching the rest, and the whole thing runs as part of your service stack or backend rather than as a separate sidecar, with agent governance and end-to-end traces handled by the same system.

This article covers a quick-picks table by workload, and a comparison table across all 9 harnesses on 8 control criteria.

## Quick picks by workload

The best agent harness for a given workload depends on where it runs, what controls you need by default, and how much of the execution layer you want to own.

The table below answers those questions per workload.

| Workload | Pick | Why |
| --- | --- | --- |
| Agents that run as part of your service stack or backend, with flexible agent governance and end-to-end traces | iii harness | AI Agent Harness with replaceable workers that run as part of your application |
| A small harness you can read and rewrite | Pi | Minimal core, extensions, and many providers |
| Open-source coding with broad model choice | OpenCode | 200+ providers |
| Local coding with a sandbox on by default | Codex CLI | OS command sandbox with command networking off by default |
| An agent with an open governance model | goose | Apache-2.0, part of the Linux Foundation's Agentic AI Foundation |
| A self-modifying agent that stores memories | Hermes Agent | Chat gateway, memory, and 6 built-in terminal backends |
| A Claude-powered agent inside your product | Claude Agent SDK | Claude Code's agent loop as a library |
| Your own agent product with human review | LangChain Deep Agents | Configurable interrupts and compatible tool-calling models |
| A self-hosted, sandboxed agent server | OpenHands Software Agent SDK | Docker and remote workspaces |

If your question is specifically about adding custom tools and choosing your own model without rebuilding the execution layer, iii, Pi, OpenCode, goose, Hermes Agent, LangChain Deep Agents, and OpenHands all document paths for both.

Claude Agent SDK documents Claude-backed providers rather than arbitrary local backends. Codex CLI defaults to OpenAI and requires additional configuration to use Ollama or LM Studio via compatible Responses endpoints.

## How we compared the harnesses

We compared each harness across 8 criteria, including model choice, tool extensibility, approval controls and their defaults, isolation, where it runs, setup, inference cost model, and license.

Every cell reflects the harness's current documentation as of November 12, 2026. We did not benchmark any harness.

The defaults column reflects what each harness does before you change any settings, because most developers evaluating a new harness start there. A permissive default and a configurable control are both important.

## Agent harnesses compared on control

*Table checked November 12, 2026, against each maker's documentation and LICENSE file. Defaults and licenses can change in later releases. Verify before adopting.*

| Harness | Type | Models | Approvals by default | Isolation | License |
| --- | --- | --- | --- | --- | --- |
| iii | Engine with harness and console | Configured provider workers, including llama.cpp | Entrypoint-dependent. Configured approval-gate holds chosen calls. rbac-proxy gates worker-level paths | Optional libkrun microVM sandboxes. Bring your own sandbox too | Engine and CLI: Elastic License 2.0. SDKs, workers, and ADE: Apache 2.0 |
| Pi | Terminal agent and SDK | Many APIs, subscriptions, local GGUF | No built-in per-tool approval | No built-in isolation | MIT |
| OpenCode | Terminal, desktop, and server | 200+ providers, local models | Allow, ask, or deny rules with a permissive base | No built-in isolation | MIT |
| Codex CLI | Terminal, IDE, and SDK | OpenAI first; Ollama or LM Studio via compatible endpoints | On-request with workspace-write, trust/config dependent | OS command sandbox with command networking normally off | Apache-2.0 |
| goose | Desktop, CLI, and server | 45+ providers | Autonomous mode by default | Optional Docker execution | Apache-2.0 |
| Hermes Agent | Terminal and chat gateway | Compatible providers, including Ollama and vLLM | Smart approval for flagged shell commands, backend-dependent | Optional Docker or cloud sandboxes; SSH is remote execution | MIT |
| Claude Agent SDK | Library | Claude through supported providers | Default denies approval-required calls without an explicit approval path | Optional Bash sandbox, off by default, shared kernel | Commercial Terms; Python MIT, TypeScript proprietary |
| LangChain Deep Agents | Library and dcode CLI | Compatible LangChain tool-calling models | No review interrupts until configured | Optional first- or third-party sandboxes | MIT |
| OpenHands Software Agent SDK | Library and agent server | LiteLLM-supported providers and compatible endpoints | ConfirmRisky by default (configurable risk threshold); NeverConfirm available but not default | Optional Docker, remote isolation deployment-dependent | MIT |

## The 9 harnesses

### 1. iii

[iii's AI Agent Harness](https://iii.dev/) runs as part of your service stack or backend, with agent governance and end-to-end tracing built into the same system. The turn loop, sessions, context, model routing, approvals, and sandboxes are separate workers on a shared engine, so you can replace any one of them without touching the others.

The agent loop runs in a durable harness worker that connects the session-manager, context-manager, and llm-router.

The whole stack uses the same invocation contract as your application's other workers. iii is a framework for building intelligent systems, with the AI Agent Harness as the practical entry point.

#### What you control

- **Models.** The llm-router worker reads provider API keys from your environment and routes completions to whichever providers you configure, including a llama.cpp provider for local inference. You swap providers by changing the router's configuration, not by rebuilding the harness.
- **Tools.** Agent tools are functions registered by workers. Any worker in your system can expose a function the harness can call, so custom tools and application logic share the same registration path. Pi, OpenCode, Claude Code, Codex, and Hermes can also run as iii workers.
- **Approvals and isolation.** Fresh sessions without an explicit profile policy deny access by default. Profiles and parentless spawns can inherit allow-all, so the entrypoint configuration determines the access baseline. The approval-gate worker intercepts chosen function calls mid-turn and holds them for a human decision before execution. Direct worker-level calls are gated separately by rbac-proxy, which sits between workers and the engine and enforces authorization on each invocation. Approval holds do not expire automatically, and the session transcript and the `approval::on-pending-resolved` event form the audit trail. For compute isolation, sandboxed workers use libkrun microVMs. On Linux, this requires read/write access to `/dev/kvm`; on macOS, it requires Apple Silicon. Any other host returns an `UnsupportedPlatform` error, so Windows, including WSL2, is not currently supported. You can also bring your own sandbox boundary.

<figure data-highlight>
  <img src="../../assets/blog/best-agent-harnesses/approval-hold.png" alt="The iii console chat view with a pending approval: the agent's agent_call request to triage::get-issue for issue number 6 is shown as JSON, with the note that execution is paused until you approve or deny this call, and approve, deny, and approve always buttons." width="1747" height="660" />
  <figcaption>An approval hold in the iii console: the function call is paused mid-turn until a developer approves or denies it.</figcaption>
</figure>

#### Trade-offs

- iii is pre-1.0, so SDK interfaces can still change between releases. Pin each worker's version in `worker-compose.yaml` to control when you take updates.
- The engine and CLI run under the Elastic License 2.0, which means you can use iii commercially but cannot offer iii itself as a service. The SDKs, workers, and console are Apache 2.0. Confirm component licensing from the LICENSE file before use in commercial products.

**Setup.** 1 command installs the engine and walks you through project setup:

```bash
curl -fsSL https://iii.dev/registry/install/iii/latest.sh | sh
```

Select llm during setup and the installer configures the harness automatically. Run `iii compose --up` to start the engine, then open the console at `http://127.0.0.1:3113`. The first time you open it, a setup wizard walks you through connecting a model provider. Provider costs come from whichever inference providers you configure. Try it with your own model using the [iii install guide](https://iii.dev/docs/install).

**Best for.** Teams running agents as part of their service stack or backend that want flexible agent governance, from approval holds to per-call authorization and entrypoint-level access policies, and end-to-end traces of every worker call in a single console.

### 2. Pi

[Pi](https://pi.dev/) is a minimal, extensible terminal agent. Its core is small enough to read in an afternoon, and the extension model means you add capabilities without forking the main codebase.

#### What you control

- **Models.** Pi supports many API providers, subscription logins, and local GGUF files. You bring your own API key or authenticate via a supported subscription.
- **Tools.** Extensions, skills, prompt templates, Pi packages, and MCP over stdio or HTTP. Pi also exposes print, JSON, and RPC modes, plus an SDK for embedding it in your own application.
- **Approvals and isolation.** Pi has no built-in per-tool approval mechanism. Project trust gates most project resources at setup time, not individual tool calls during a run. Extensions can add approval gates, and you can add a container or VM for containment, but neither is provided by default.

#### Trade-offs

- Pi has no built-in per-tool approvals or sandbox, so you're responsible for adding both. For any workload touching production resources, add an external approval layer and run Pi inside a container.
- Oh My Pi, a separate fork by Stencil Labs, is worth checking if you want opinionated defaults on top of Pi's core.

**Best for.** Developers who want to own the loop and will build their own guardrails.

### 3. OpenCode

[OpenCode](https://opencode.ai/) is an open-source coding agent with a client/server design and 200+ provider integrations.

Its permission system uses rules: every tool call resolves to allow, ask, or deny, and the last matching rule wins.

#### What you control

- **Models.** 200+ providers, local models, and self-hosted endpoints. OpenCode connects to whichever provider you configure with an API key.
- **Tools.** Plugins, custom tools, MCP, and skills. In V2, `opencode serve` runs OpenCode headless. Set `OPENCODE_SERVER_PASSWORD` to require HTTP Basic Auth. If you leave it unset, the server runs unauthenticated and displays a warning. There is no auto-generated password.
- **Approvals and isolation.** The rule system resolves every action to allow, ask, or deny, with the last match winning. The base configuration is permissive. V2 allows access to external directories unless you add an explicit deny rule, so the defaults require attention before you use OpenCode on shared or production machines. There is no built-in sandbox.

#### Trade-offs

- Permissive defaults and no sandbox mean you need to add rules and a containment boundary before connecting OpenCode to sensitive resources.
- OpenCode V1 and V2 use different field and action names (`permission/bash/task` vs. `permissions/shell/subagent`); the current permissions doc covers V2 behavior.

**Best for.** Terminal coding workflows that need broad model choice and rule-based approvals.

### 4. Codex CLI

[Codex CLI's](https://learn.chatgpt.com/docs/codex/cli) default sandbox restricts command networking and workspace writes for shell commands, though it does not block all reads or client traffic.

Sandbox enforcement depends on trust level, configuration, and OS.

#### What you control

- **Models.** OpenAI-first. Use `codex --oss` with compatible Ollama or LM Studio Responses endpoints; `oss_provider` selects the default non-OpenAI provider. Other providers require user-level configuration beyond the default setup.
- **Tools.** Hooks such as `PreToolUse`, MCP, skills, and subagents. `codex exec` supports CI use cases and the Codex SDK covers library embedding.
- **Approvals and isolation.** The sandbox and approval policy are separate settings, so you can combine workspace-write restrictions with on-request approvals. The default sandbox restricts command networking and writes on supported platforms, but trust level and OS affect what actually gets enforced.

#### Trade-offs

- OpenAI-first by design. Local model support exists but requires extra configuration steps that the default setup does not handle.
- Sandbox enforcement varies by OS and trust setting. Verify the actual boundary for your platform before treating the default as a hard isolation guarantee.

**Best for.** Local coding where a default sandbox reduces the blast radius before you configure anything else.

### 5. goose

[goose](https://goose-docs.ai/) is an agent with an open governance model. It runs as a desktop app, CLI, or server and joined the Linux Foundation's Agentic AI Foundation as an Apache-2.0 project.

Neutral foundation governance and a permissive license give you more assurance about the project's long-term direction than a solo-vendor project.

#### What you control

- **Models.** 45+ providers, configured via API key, browser OAuth, or subscription. Some subscription paths, such as Claude Code and Codex, connect over ACP; others use direct OAuth.
- **Tools.** 70+ MCP extensions, an optional installation allowlist to control which extensions load, and recipes for reusable workflows. Manual and Smart approval modes support per-tool Always Allow, Ask Before, or Never Allow policies.
- **Approvals and isolation.** goose defaults to Autonomous mode, which runs without per-tool confirmation. Manual and Smart modes add per-tool approval controls, but you have to switch modes explicitly. Docker isolation exists but requires setup and is not on by default.

#### Trade-offs

- Autonomous mode is the default. If you connect goose to tools with write access or external effects, switch to Manual or Smart mode before your first real run.
- Docker isolation requires explicit configuration.

**Best for.** Teams that want an agent with an open governance model, backed by a neutral foundation rather than a single vendor, and who will switch away from Autonomous mode before connecting real tools.

### 6. Hermes Agent

[Hermes Agent](https://hermes-agent.nousresearch.com/) is a self-modifying agent that stores memories. It writes new skills after complex tasks, refines them as it uses them, and keeps persistent memory and a profile of you across sessions. You host it, and a chat gateway connects it to Telegram, Discord, Slack, WhatsApp, and Signal.

It also runs scheduled tasks and supports 6 terminal backends for different execution environments.

#### What you control

- **Models.** Compatible providers including Ollama and vLLM, plus API-key providers. You configure the provider per deployment.
- **Tools.** Plugins, MCP, skills, and hooks. Terminal backends: local, Docker, SSH, Modal, Daytona, and Vercel Sandbox. Each backend has different isolation characteristics.
- **Approvals and isolation.** Smart approval checks flagged shell commands on local and SSH backends. Docker and cloud backends can bypass that check depending on configuration. Local execution is unsandboxed. The backend determines the level of isolation, so check its documentation for your deployment environment.

#### Trade-offs

- Local execution has no sandbox. Use a Docker or cloud backend if containment is a requirement.
- Smart approval applies to flagged commands and is not a universal tool-call gate.

**Best for.** A personal or team agent that remembers across sessions and builds up its own skills over time, reachable from messaging platforms.

### 7. Claude Agent SDK

The [Claude Agent SDK](https://code.claude.com/docs/en/agent-sdk/overview) provides the Claude Code agent loop as a Python or TypeScript library.

You embed it in your own product and get access to Claude's planning, tool use, subagents, and in-process MCP support without maintaining the loop yourself.

#### What you control

- **Models.** The SDK runs Claude through supported providers: Anthropic API, Bedrock, Vertex, or Foundry. It runs a Claude Code binary bundled by default and is documented for Claude, not arbitrary local backends.
- **Tools.** In-process MCP tools, subagents, and hooks. The `canUseTool` callback and rules-based hooks let you gate which tool calls execute.
- **Approvals and isolation.** From runtime 2.1.302 onward, approval-required calls are denied by default unless you configure an approval path through rules, hooks, or `canUseTool`. Check which runtime version your release bundles. The optional Bash sandbox uses bubblewrap on Linux and sandbox-exec on macOS, but it shares the kernel and is off by default.

#### Trade-offs

- Designed for Claude. If your product requires model flexibility, this SDK will constrain you.
- The Bash sandbox shares the kernel, so it provides process isolation rather than full OS-level containment. Pair it with a VM or container for stronger isolation.
- Licensing is split by component: Python SDK is MIT, TypeScript SDK is proprietary, and Commercial Terms apply to SDK use with documented exceptions. Read the license for each component before shipping.

**Best for.** Products committed to Claude that want its agent loop without building one from scratch.

### 8. LangChain Deep Agents

[LangChain Deep Agents](https://www.langchain.com/deep-agents) is a harness library on LangGraph. It adds planning, a filesystem, sub-agents, and skills to LangGraph's graph execution model.

Human-in-the-loop controls use LangGraph's interrupt and checkpointer mechanisms.

#### What you control

- **Models.** Compatible LangChain tool-calling models. You configure the model and credentials in your application code.
- **Tools.** Configurable interrupts, a checkpointer, and thread ID let you approve, edit, reject, or respond to pending actions. Python lets you add per-call conditions on top of that. Sandbox backends: LangSmith, Daytona, E2B, Modal, Runloop, and Vercel. A separate dcode CLI handles development workflows.
- **Approvals and isolation.** No review interrupts are active until you configure them. Human-in-the-loop controls are a configuration layer, not a default. Sandbox backends are optional, including LangSmith.

#### Trade-offs

- Library-first: you wire it into your application rather than running a standalone agent server. That's the right fit for products but adds work for quick experiments.
- LangChain also offers managed deployment, which is a separate surface from the library itself.

**Best for.** Python or JavaScript teams building their own agent product with configurable human review and model choice.

### 9. OpenHands Software Agent SDK

[OpenHands Software Agent SDK](https://docs.openhands.dev/sdk) pairs a library with an agent server for running sandboxed agents you host.

It supports local, Docker, and remote workspaces, and its confirmation policy system lets you control which actions require approval.

#### What you control

- **Models.** LiteLLM-supported providers and compatible endpoints. API keys come from your environment.
- **Tools.** Custom tools, MCP, skills, and hooks. The workspace layer controls where the agent executes: local, Docker, or remote.
- **Approvals and isolation.** According to the SDK's architecture docs, ConfirmRisky is the default confirmation policy, not NeverConfirm. It uses a configurable risk threshold to auto-confirm low-risk actions and hold high-risk actions for approval. NeverConfirm (skip all confirmation) and AlwaysConfirm are also available. Docker isolation is opt-in, not on by default. Remote workspace isolation depends on your deployment configuration.

#### Trade-offs

- A local workspace has no sandbox. If isolation is a requirement, configure Docker or a remote workspace before connecting the agent to real resources.

**Best for.** Platform teams that want a self-hosted agent server with container-backed workspaces.

## What the control table does not show

**Check the defaults before you trust any harness with real tools.** goose starts in Autonomous mode, which runs without per-tool confirmation. Hermes Agent's smart approval catches only flagged shell commands, and the check applies differently across its 6 backends.

OpenCode's permission system defaults to allow for most actions in V2, so external directories are accessible unless you add a deny rule.

Pi has no built-in per-tool approval mechanism and needs an extension to add one.

The table shows what each harness supports when configured, and the defaults column shows what actually runs before you configure anything.

**Approvals and sandboxes are separate controls, and both matter.** [Anthropic describes its critical-path check](https://platform.claude.com/docs/en/agents) as a permission gate, not a sandbox. The optional Bash sandbox in the Claude Agent SDK shares the kernel, which limits what it can actually contain.

Approval gates stop the agent from calling a tool without confirmation, and a confirmed tool call can still escape a process boundary, so for workloads where a confirmed action could cause damage, pair approval controls with a VM or container.

See [Why agent sandboxes are converging on libkrun, not Firecracker](https://iii.dev/blog/why-agent-sandboxes-are-converging-on-libkrun-not-firecracker/) for a detailed look at how sandbox isolation actually works.

**Test results depend on the model, the task set, and the harness configuration.** Composio's vendor-run test used 1 model across 25 business-workflow tasks.

Claude Code and Codex CLI ran with a non-native model in that test, so the results for those harnesses reflect neither their native-model combination nor a coding benchmark.

That 20-percentage-point spread comes from a constrained test. Evaluate the harnesses on your own tasks before treating its ranking as a general recommendation.

**Check the license for each component, not the project name.** The iii engine and CLI are under the Elastic License 2.0, while its SDKs, workers, and console are Apache 2.0.

The Claude Agent SDK has a component licensing split: Python MIT, TypeScript proprietary, Commercial Terms for SDK use. LangChain Deep Agents is MIT, but LangSmith (the managed observability and sandbox service) has its own pricing.

OpenHands Software Agent SDK is MIT end to end, with no comparable commercial-terms overlay.

Read the LICENSE file for every component you plan to ship, not just the repository top-level license. See also [What stops your coding agent from doing something it can't undo?](https://iii.dev/blog/what-stops-your-coding-agent-doing-something-it-cant-undo/) for a practical breakdown of which controls actually prevent irreversible actions.

## What else to know about agent harnesses

A few questions come up consistently when developers move from hosted agents to self-managed harnesses.

### What is the difference between an agent harness and an agent framework?

An AI Agent Harness supplies a runnable loop, the execution structure that calls the model, routes tool calls, and manages turns. A framework supplies building blocks you assemble into a loop yourself.

The categories overlap: some projects market themselves as frameworks but ship a runnable harness, and some harnesses expose enough extension points that you can rebuild their internals.

For approvals and isolation, the defaults of a given harness are your starting point, while a framework's defaults only come into play once you've wired everything together.

[Anthropic's guide to effective harnesses for long-running agents](https://platform.claude.com/docs/en/agents) is a good primary reference on what a production harness needs to handle.

### Which coding agent has the best harness?

It depends on the model and the task. Composio's single-model business-workflow test found a 20-percentage-point spread across harnesses, which is large enough to change which one you'd pick.

That test doesn't tell you how the harnesses compare on your codebase with your preferred model and approval configuration. Test your top 2 or 3 candidates on a representative sample of your own tasks before committing to one.

### Can I run these harnesses with a local model?

The 8 harnesses other than the Claude Agent SDK document local or self-hosted provider paths. iii connects through its llm-router worker with a llama.cpp provider. Pi accepts local GGUF files.

OpenCode supports local models through its provider configuration. Codex CLI uses `codex --oss` with a compatible Ollama or LM Studio endpoint. goose routes through its provider system, which includes local options.

Hermes Agent supports Ollama and vLLM. LangChain Deep Agents accepts any compatible LangChain tool-calling model, including locally hosted ones. OpenHands uses LiteLLM, which proxies local endpoints.

The Claude Agent SDK is documented for Claude-backed providers rather than arbitrary local inference.
