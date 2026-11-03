---
title: 'Pi vs OpenCode vs iii for extensible coding workflows'
seoTitle: 'Pi vs OpenCode vs iii: Extending Your Coding Workflow'
description: 'We tested the same tool with Pi, OpenCode, and iii to compare setup, error handling, and reuse between coding agents and applications.'
pubDate: 2026-11-05
tags: ['agents', 'comparison', 'pi', 'opencode', 'mcp', 'harness']
ogImage: ../../assets/blog/pi-vs-opencode-vs-iii/og.png
faq:
  - q: 'Does Pi support MCP servers now?'
    a: 'Yes. Pi added MCP with Codemode in v0.99.0 on September 29, 2026, and Pi 1.0 ships it. Earlier reviews contrasted OpenCode''s built-in MCP with Pi''s extension-first core. That distinction is now historical, as both ship MCP as a first-class configuration path.'
  - q: 'Can I embed Pi or OpenCode in my own app?'
    a: 'Yes, in different shapes. Pi offers a TypeScript SDK and an RPC mode over stdin and stdout. OpenCode offers a client for its background service''s HTTP API. In iii, the AI Agent Harness is itself a Worker on the same engine as your application''s Workers, so the integration boundary is a Function call rather than a separate protocol.'
  - q: 'How do I share an extension with my team?'
    a: 'Pi packages bundle extensions, skills, and prompt templates from npm or git, and versions can be pinned. OpenCode plugins install from npm or git and are listed in opencode.json(c). iii Workers come from the registry via compose::add and are pinned in worker-compose.lock.'
---

![Pi vs OpenCode vs iii: one tool, two callers. An Agent and an App both connect to one iii Function, release-notes::get.](../../assets/blog/pi-vs-opencode-vs-iii/banner.png)

You've built a small HTTP service that returns release notes for versioned packages, and now both your coding agent and your application need to call it.

Every team building real workflows hits this extension decision eventually, and it's where the top Pi vs OpenCode comparisons go silent.

The top-ranking comparison pages contrast OpenCode's built-in MCP with Pi's extension-first core, but none of them adds the same tool to both agents and asks what happens when the application also needs that tool.

In this article, we built one release-notes lookup as a standalone HTTP service, connected it with Pi, OpenCode, and iii, and tested how each tool works when your agent and your app both need the same tool.

> **Versions tested, checked October 13, 2026:** Pi 1.0.4, OpenCode v2.0.24, and iii engine 0.24.4 with AI Agent Harness 1.8.44, all running against the same claude-sonnet-5 model via the same Anthropic API key.

## Our quick take by workload

Pi and OpenCode are coding agents you extend with MCP servers or plugins. iii is a framework where the agent and the application call the same Functions. The right choice depends on which problem you're actually solving.

<div class="verdict">
  <div class="verdict-card">
    <p class="verdict-pick">Best for getting the tool call right with minimal config</p>
    <p class="verdict-tool">Pi</p>
    <p class="verdict-reason">The .pi/mcp.json wiring was the smallest config of the three (one pi mcp add command, one JSON file), and the agent used the tool correctly with no missteps in our run.</p>
    <a class="verdict-cta" href="https://iii.dev/docs/install">Pi in iii</a>
  </div>
  <div class="verdict-card">
    <p class="verdict-pick">Best for a full agent with plugins and MCP built in</p>
    <p class="verdict-tool">OpenCode</p>
    <p class="verdict-reason">OpenCode ships agents, commands, skills, MCP, and a plugin API in one install, so you're customizing one complete tool rather than assembling pieces.</p>
    <a class="verdict-cta" href="https://iii.dev/docs/install">OpenCode in iii</a>
  </div>
  <div class="verdict-card" data-highlight>
    <p class="verdict-pick">Best when the app needs the tool too</p>
    <p class="verdict-tool">iii</p>
    <p class="verdict-reason">Confirmed in our run: the app called the exact same release-notes::get Function the agent did, with no second implementation, and both calls appeared in the same filterable trace list in the console.</p>
    <a class="verdict-cta" href="https://iii.dev/docs/install">Start the harness</a>
  </div>
</div>

## How we ran the test

We built one release-notes lookup as a standalone HTTP service returning merged changes for a `beacon-cli` version from a local fixture, with a clean 404 for unknown versions. We connected it three ways: a stdio MCP server used identically by Pi and OpenCode, and a Function registered by an iii Worker.

All three call into the same running service, so every result below reflects how each option exposes and reaches a tool.

We ran four cases per tool using the same fixed prompt template: "Look up the release notes for beacon-cli version {VERSION} using the available tool, and summarize the changes in one sentence."

<div class="compare">

| Setup | Pi | OpenCode | iii |
| --- | --- | --- | --- |
| Version tested | 1.0.4 | 2.0.24 | Engine 0.24.4, AI Agent Harness 1.8.44 |
| Where the tool runs | MCP server process | MCP server process | Worker process |
| How the agent finds it | `.pi/mcp.json`, exposure `direct` | `mcp.servers` in `opencode.jsonc`, `codemode: false` | AI Agent Harness allow list (`options.functions.allow`) |
| How the app calls it | A separate script (`app-client/call-release-notes.mjs`), its own plain HTTP call with no shared code with the agent's MCP server | Same separate script, same way | `iii trigger release-notes::get`, the identical registered Function the AI Agent Harness exposes to the agent |
| Model and key | claude-sonnet-5, Anthropic API key | Same | Same |

</div>

## Two coding agents you extend and one framework you build on

All three options can call a tool running outside the agent's own process. The difference is how you declare the tool, who else can call it, and what you own after it's working.

The sections below cover where custom tools run and how the application reaches them. For approval gating and permission policy, see [OpenCode vs Claude Code vs iii for human in the loop workflows](https://iii.dev/blog/opencode-vs-claude-code-vs-iii/).

### Pi

Pi is Earendil's MIT-licensed "minimal, extensible agent harness." Pi 1.0 shipped October 1, 2026, and extensions are TypeScript modules that load without a build step. Of the three options, Pi had the smallest config surface to go from a running service to an agent that could call it.

#### Key facts

<div class="facts">

- **Version tested:** 1.0.4
- **License:** MIT
- **Where custom tools run:** an MCP server process started by Pi via `.pi/mcp.json` (the route used in this test), or inside the Pi process itself via `pi.registerTool` and `pi.registerMcpServer` (not exercised here, per Pi's [Extensions docs](https://pi.dev/docs/latest/extensions))
- **How the app reaches the same tool:** a separate script making its own plain HTTP call, with zero code shared with the MCP server
- **How extensions are shared:** Pi packages install from npm or git, with versions that can be pinned, per Pi's [Packages docs](https://pi.dev/docs/latest/packages)

</div>

#### Why it stands out

`pi mcp add <server> -l --exposure direct -- <command>` wrote a working, trusted-on-first-run MCP config in one command. Pi was the only tool of the three where wiring the shared service in was a single CLI invocation rather than a JSON block we hand-edited.

The `tool_call` handler in Pi's extension API can also change tool input or block execution before the model sees the result, per Pi's docs, though we didn't exercise that path in this run.

#### Limitation

Project-local MCP config is untrusted by default and silently ignored until the project is trusted, confirmed directly.

Our first `pi mcp list` attempt showed nothing configured until we ran `pi --approve`. Extensions run inside the Pi process with the host OS's permissions. Pi's [Security docs](https://pi.dev/docs/latest/security) point to a container, VM, or sandbox for real isolation, with no built-in boundary.

#### Best for

Developers who want a small config surface to get one shared tool working and are comfortable writing the application's own call path separately. Follow [the iii install guide](https://iii.dev/docs/install) to start the AI Agent Harness and console on your machine.

<figure>
  <img src="../../assets/blog/pi-vs-opencode-vs-iii/pi-session-success.png" alt="Pi's rendered session transcript: the prompt asks for beacon-cli 2.6.0 release notes, the mcp__release_notes__get_release_notes call returns three changes, and the assistant summarizes them in one sentence." width="2048" height="1090" />
  <figcaption>Pi's rendered session transcript for a successful agent call.</figcaption>
</figure>

<figure>
  <img src="../../assets/blog/pi-vs-opencode-vs-iii/pi-session-unknown-version.png" alt="Pi's rendered session transcript for beacon-cli 9.9.9: the tool call returns &quot;no release notes for version 9.9.9&quot; and the assistant says the version doesn't appear to exist." width="2048" height="1088" />
  <figcaption>The unknown-version case in Pi: the tool call returns "no release notes for version 9.9.9", and Pi reports it plainly without guessing or retrying.</figcaption>
</figure>

### OpenCode

OpenCode is Anomaly's MIT-licensed coding agent. OpenCode 2 ships agents, commands, skills, MCP, and a plugin API built on transforms and hooks.

Of the three, OpenCode gave us the fastest live config turnaround, and the only unexpected model behavior worth calling out.

#### Key facts

<div class="facts">

- **Version tested:** v2.0.24
- **License:** MIT
- **Where custom tools run:** an MCP server process declared in `opencode.jsonc` with `codemode: false` (the route used in this test), or inside an OpenCode plugin registered via `ctx.tool.transform` (per OpenCode 2's [Build plugins docs](https://opencode.ai/v2/docs/plugins))
- **How the app reaches the same tool:** the same separate script used for Pi, a plain HTTP call to the service with no shared code with the agent's MCP connection
- **How plugins are shared:** plugins install from npm or git and are listed in `opencode.json(c)`, per OpenCode 2's [Configure plugins docs](https://opencode.ai/v2/docs/plugins)

</div>

#### Why it stands out

A new MCP server entry took effect after `opencode reload`, confirmed via `opencode mcp list` showing the new server connected.

OpenCode also produced the only unexpected model behavior in the test. On the known-version prompt, the agent reached for its own built-in web search tool before the correctly configured `release-notes_get_release_notes` tool.

The web search errored (no search capability configured in the sandbox), and OpenCode then correctly fell back to the right tool and succeeded. Registering a tool doesn't guarantee the model picks it first.

#### Limitation

OpenCode 1 plugins do not run in OpenCode 2, per OpenCode 2's [Migrate from V1 docs](https://opencode.ai/v2/docs/migrate-v1).

#### Best for

Teams that want a complete agent and plan to iterate on its config or plugins during a live session, without restarting a background service. Follow [the iii install guide](https://iii.dev/docs/install) to start the AI Agent Harness and console on your machine.

<figure>
  <img src="../../assets/blog/pi-vs-opencode-vs-iii/opencode-session-success.png" alt="OpenCode's session view for beacon-cli 2.6.0: the tool summary reads &quot;Used 2 Web Search, release-notes_get_release_notes&quot;, followed by a one-sentence summary of the release." width="2048" height="1010" />
  <figcaption>OpenCode's session view for a successful agent call. It used Web Search first (which errored, with no search capability configured) before correctly falling back to release-notes_get_release_notes.</figcaption>
</figure>

<figure>
  <img src="../../assets/blog/pi-vs-opencode-vs-iii/opencode-session-unknown-version.png" alt="OpenCode's session view for beacon-cli 9.9.9: the release-notes_get_release_notes call shows a red &quot;No release notes for version 9.9.9&quot; error, and the model says the version likely doesn't exist." width="2048" height="1005" />
  <figcaption>The unknown-version case in OpenCode: the tool call shows a red "No release notes for version 9.9.9" error badge, and the model relays it without guessing or retrying.</figcaption>
</figure>

### iii

> **Disclosure.** iii is our product, included here because it answers the same question from the application side.

iii is a framework for building intelligent systems. Functionality is built once, and the AI Agent Harness, Triggers, and application code all call it.

#### Key facts

<div class="facts" data-highlight>

- **Versions tested:** engine 0.24.4, AI Agent Harness 1.8.44
- **License:** engine and CLI under Elastic License 2.0 (not OSI-approved); SDKs, console, and published workers under Apache 2.0
- **Where custom tools run:** a process registered via `worker-compose.yaml`
- **How the app reaches the same tool:** `iii trigger release-notes::get`, calling the identical Function the AI Agent Harness exposes to the agent. Workers are added via `iii trigger compose::add` and pinned in `worker-compose.lock`. Functions can be [registered](https://iii.dev/docs/using-iii/functions) from the Node, Python, Rust, or Go SDK

</div>

#### Why it stands out

iii was the only option of the three where the "app call" and "agent call" tests used the same Function definition rather than a second hand-written script.

Both calls appeared side by side in one trace view in the console, filterable by function name, each with its own nested span for the outbound HTTP call to the release-notes service.

The console's Traces page showed the agent's call nested inside the AI Agent Harness turn and the app's call as its own top-level trace from `iii-node`, making the full call history of that Function visible in one place.

#### Limitation

Worker code changes require an explicit `iii trigger compose::restart worker=release-notes` before the new behavior is visible. On Windows, bundle Workers need WSL2 with `/dev/kvm`.

#### Best for

Teams building an application where agents and code share tools. In Pi and OpenCode, a custom tool is something the agent connects to. In iii, it's first-class functionality.

In iii, a Function is something the application already owns, and the agent is one more caller of it. Follow [the iii install guide](https://iii.dev/docs/install) to start the AI Agent Harness and console on your machine.

## What happened when the agent and the app shared one tool

### Where the tool runs and who else can call it

The MCP route (`.pi/mcp.json`, `exposure: direct`) is what the Pi agent uses. The application couldn't reuse that connection and needed its own code path that shares zero code with the MCP server.

The underlying HTTP call is identical either way, so we wrote it once and reused it for both Pi's and OpenCode's app path.

OpenCode's agent MCP config (`opencode.jsonc`, `codemode: false`) is agent-only. The application used the identical separate script Pi's app path used, making a plain HTTP call to the service.

With iii, the Worker registers `release-notes::get` once. The AI Agent Harness's allow list exposes it to the agent, and `iii trigger` exposes the exact same Function to the application. We confirmed this in the console's trace list, where both call paths appear under the same function name: one nested inside a `harness::send` trace (worker: `harness`), the other as its own top-level trace (worker: `iii-node`).

<figure data-highlight>
  <img src="../../assets/blog/pi-vs-opencode-vs-iii/iii-trace-success.png" alt="The iii console with chat and traces side by side: the agent summarizes beacon-cli 2.6.0, and the trace waterfall shows release-notes::get with a child GET /release-notes/2.6.0 span inside the harness::send trace." width="2048" height="1090" />
  <figcaption>The iii console's trace view for a successful agent call: release-notes::get runs nested inside the AI Agent Harness turn, with its outbound GET /release-notes/2.6.0 request captured as a child span.</figcaption>
</figure>

### What the model saw when the tool failed

Stopping the release-notes service mid-session and asking for a known version produced the same shape of answer from all three agents: a clear unreachable explanation, no retry, no fabricated changelog.

Pi's tool result carried `isError: true` with the text "release-notes service is unreachable at http://127.0.0.1:4455: fetch failed". The model relayed that plainly and asked whether to retry.

OpenCode's tool result carried the identical underlying error text. The model explained it and offered an alternative (a GitHub repo link) rather than guessing.

iii's agent path returned a `function_result` with `is_error: true`. The AI Agent Harness turn's own result explained the same failure in one sentence. The application path returned a structured `{code, message, stacktrace}` error from the CLI, a different shape than the agent received.

Asking for an unknown version (9.9.9) with the service running produced equally clean failures from all three. "No release notes for version 9.9.9" surfaced to the model via the tool or function result in each case, and all three explained it rather than inventing a changelog.

**Where arguments get checked.** All three tools pass the version argument to our code without prior validation. The request schema registered with each (Pi's tool schema, OpenCode's MCP tool schema, iii's `request_format`) is descriptive only. The actual string-type and non-empty check happens inside our own handler, consistent with what MCP's protocol does for Pi and OpenCode and confirmed directly for iii in this run.

**Where each failure is visible afterward.** For Pi, the saved session JSONL (exportable to HTML via `pi --export`). For OpenCode, the session view in its paired browser UI, or the one-shot JSON event stream at call time. For iii, the console's Traces page for that call, including the nested outbound HTTP request that actually failed or returned a 404.

<figure data-highlight>
  <img src="../../assets/blog/pi-vs-opencode-vs-iii/iii-trace-unknown-version.png" alt="The iii console for beacon-cli 9.9.9: the agent says no release notes exist, and the trace waterfall flags release-notes::get and its child GET /release-notes/9.9.9 span as errors." width="2048" height="1093" />
  <figcaption>The same trace view for a failed agent call: release-notes::get (version 9.9.9) and its nested GET /release-notes/9.9.9 request are both flagged as errors.</figcaption>
</figure>

_Table tested on 2026-10-13._

<div class="compare">

| Case | Pi | OpenCode | iii |
| --- | --- | --- | --- |
| Agent call works | Succeeded, with a correct one-sentence summary | Succeeded, after first trying (and failing at) its own built-in web search, then producing a correct summary | Succeeded, with a correct one-sentence summary nested inside the AI Agent Harness turn's trace |
| App call works | Succeeded via the app's own separate script (HTTP 200 + JSON) | Same separate script, same result | Succeeded via `iii trigger release-notes::get`, the same Function the agent calls |
| Service stopped | Clean "unreachable" error relayed to the user, no retry | Clean "unreachable" error relayed, with an alternative offered, no retry | Agent: clean one-sentence failure explanation. App: structured `{code, message, stacktrace}` error from the CLI |
| Unknown version | Clean "no release notes for version 9.9.9" relayed, no guessing | Same, with a typo/non-existent-version explanation, no guessing | Agent: clean failure explanation. App: same structured error shape as the service-stopped case |
| Where the failure shows up | The session JSONL or its HTML export | The session view in OpenCode's paired browser UI, or the JSON event stream at call time | The console's Traces page for that call, including the specific failed or 404'd outbound HTTP request |

</div>

### What you maintain after it works

With Pi, you hand-maintain one file, `.pi/mcp.json`, plus the application's own separate script. A change to the MCP server list is picked up by `/reload` inside an active session (documented, though not directly exercised in `--print` mode, which has no session to send `/reload` into).

With OpenCode, you maintain `opencode.jsonc`, plus the same separate application script. A new MCP server entry takes effect after `opencode reload`, with no service restart.

With iii, you maintain the `worker-compose.yaml` entry plus the worker's own directory (`worker.js`, `package.json`, `iii.worker.yaml`). A code change inside the worker did not take effect until `iii trigger compose::restart worker=release-notes` was run. Querying immediately after the edit, before the restart, still returned the old behavior. With additional configuration, iii does support file-watching in worker start scripts via `tsx watch` or equivalent.

_Table tested on 2026-10-13._

<div class="compare">

| Lifecycle | Pi | OpenCode | iii |
| --- | --- | --- | --- |
| Extension code you own | One MCP config entry + the app's own script | One MCP config entry + the same app script | A worker directory (`worker.js`, `package.json`, `iii.worker.yaml`) + one `worker-compose.yaml` entry |
| Where it is configured | `.pi/mcp.json` or extension | `opencode.jsonc` or plugin | `worker-compose.yaml` |
| How a change loads | `/reload` in an active session (documented, not directly exercised in `--print` mode) | `opencode reload`, confirmed live, no service restart | `iii trigger compose::restart worker=<name>`, confirmed required, with no automatic hot reload for worker code without a file-watching start script |
| What the last major version broke | Move to 1.0 after MCP landed in v0.99.0 (not re-tested, per maker changelog) | V1 plugins do not run in V2 (not re-tested, per maker docs) | `iii worker add` removed in engine 0.23, replaced by `compose::add` |
| How a teammate installs it | Pi package or `.pi/mcp.json` | Plugin package or `opencode.jsonc` | `worker-compose.yaml` and lock |

</div>

## Process separation and security boundaries

Separate processes don't create security boundaries. All three tools behaved the same way in both failure and success cases, regardless of process isolation. The service being a separate process changed what crashed when we stopped it (the service alone, not the agent or the AI Agent Harness), but it didn't change what any of the three tools' own code could reach.

Pi's [docs](https://pi.dev/docs/latest/security) say extensions run with the same OS permissions as Pi itself and point to a container, VM, or sandbox for real isolation.

OpenCode's [docs](https://opencode.ai/v2/docs/permissions) warn that shell commands run with the host user's own filesystem, process, and network authority.

iii's rbac-proxy Worker decides which callers may invoke which Functions, but it is not an isolation boundary. Sandbox calls and Compose bundles run in VMs, but a local Worker like the one in this test runs on the host unless configured otherwise, per the [iii Sandboxes docs](https://iii.dev/docs/creating-workers/sandboxes).

Choose where a tool lives based on reuse and failure handling, which is what this test measured, then choose isolation separately. For approval gating and how to control what a coding agent can do before it does it, see [What stops your coding agent from doing something it can't undo?](https://iii.dev/blog/what-stops-your-coding-agent-doing-something-it-cant-undo/)

## Where iii fits

You can keep Pi or OpenCode and add iii around them. [The pi worker](https://github.com/iii-hq/workers/tree/main/pi) exposes Pi as `pi::run` and [the opencode worker](https://github.com/iii-hq/workers/tree/main/opencode) exposes OpenCode as `opencode::run`, mirroring each agent's lifecycle events into iii's engine so the console traces them alongside native AI Agent Harness turns.

[The mcp worker](https://github.com/iii-hq/workers/tree/main/mcp) can expose iii Functions to MCP clients, which now include Pi and OpenCode. We describe this as a compatibility path: tools already built as iii Functions can be surfaced to agents that connect via MCP.

iii supplies the framework and routes invocations across Workers, with inference coming from the provider you connect, whether Anthropic, OpenAI, or another.

## What else to know before you choose

### Does Pi support MCP servers now?

Yes. Pi added MCP with Codemode in [v0.99.0 on September 29, 2026](https://pi.dev/changelog), and Pi 1.0 ships it. Earlier reviews, including the AI Overview for the primary search term at research time, contrasted OpenCode's built-in MCP with Pi's extension-first core. That distinction is now historical, as both ship MCP as a first-class configuration path.

### Can I embed Pi or OpenCode in my own app?

Yes, in different shapes. Pi offers a [TypeScript SDK](https://pi.dev/docs/latest/sdk) and an [RPC mode](https://pi.dev/docs/latest/rpc) over stdin and stdout. OpenCode offers a client for its background service's HTTP API, per OpenCode 2's client docs. In iii, the AI Agent Harness is itself a Worker on the same engine as your application's Workers, so the integration boundary is a Function call rather than a separate protocol.

### How do I share an extension with my team?

Pi packages bundle extensions, skills, and prompt templates from npm or git, and versions can be pinned, per Pi's [Packages docs](https://pi.dev/docs/latest/packages). OpenCode plugins [install from npm or git](https://opencode.ai/docs/plugins) and are listed in `opencode.json(c)`. iii Workers come from the registry via `compose::add` and are pinned in `worker-compose.lock`.
