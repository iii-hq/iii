---
title: 'How to Build Your Own Agent Harness'
description: 'Most agent teams adopt a harness as one decision. iii decomposes it into eleven independently-replaceable workers on one engine bus. Build your own stops meaning fork a framework and starts meaning swap a few workers.'
pubDate: 2026-05-28
author: 'Mike Piccolo, Founder & CEO of iii'
tags: ['agents', 'harness', 'architecture', 'workers', 'orchestration']
ogImage: ../../assets/blog/how-to-build-your-own-agent-harness/cover.png
---

![How to Build Your Own Agent Harness — loops, tools, memory, sandbox, and observability](../../assets/blog/how-to-build-your-own-agent-harness/cover.png)

Most agent teams don't build a harness. They adopt one. LangChain, LangGraph, OpenAI Agents SDK, Anthropic SDK, CrewAI, AutoGen, the loop, the tools, the memory, the orchestration are picked off the shelf as a single decision. The harness is a framework you import. If something inside it doesn't fit, **you fork it, fight it, or work around it.**

I think that shape is **wrong**, and it's the reason every long-running agent team eventually ends up rewriting its harness from scratch. The harness isn't one thing. It's ten or twelve different things bundled together because the surrounding ecosystem doesn't give you a way to compose them. [**Pi agent**](https://github.com/earendil-works/pi/tree/main/packages/coding-agent) **packages** are on the right track, but they are still in the paradigm of “Add another service and integrate it with all others” The iii engine treats all workers the same and removes the integration logic completely. The **provider router, the credential vault, the policy engine, the approval gate, the model catalog, the session storage, the budget tracker, the after-call hook fanout, the durable turn loop** are independent concerns. These are all interoperable with your queue, http/api server, streaming, even browser workers. A framework that ships them as one block is selling you a tradeoff you didn't have to make.

The bet underneath [iii](https://iii.dev/) is that they shouldn't be one block. There should be a set of workers on a shared engine, each replaceable, each versioned independently, each connected by a single primitive: **a trigger (`iii.trigger()`)** that every other worker also uses. The harness becomes a stack of installable workers, and "**build your own**" stops meaning "**fork a framework.**" It means "**swap a few workers.**"

This post walks through what that actually looks like. The complete stack that drives an iii agent turn today, why each layer is its own worker, and how you replace any of them.

## **The 15 jobs an agent harness has to do**

If you strip a production agent harness back to its responsibilities, you get a list that looks roughly like this:

1. Accept a turn request from a client and persist it  
2. Resolve credentials for whichever model provider gets called  
3. Look up what the chosen model can actually do (vision, tools, streaming, context window)  
4. Drive the per-turn state machine, provision, stream assistant, run tools, steer, tear down  
5. Load and serve skill bodies that describe each function's request shape, error codes, and usage notes  
6. Assemble the system prompt, mode paragraph, identity preamble, working directory, and default skills appendix  
7. Stream tokens back to the client as the model produces them  
8. Check every tool call (that’s just a function) against a policy before it runs  
9. Pause tool calls that need a human decision and route the answer back to the right turn  
10. Track LLM spend against per-workspace or per-agent budgets  
11. Run hooks before and after tool calls (logging, redaction, custom side effects)  
12. Persist the session as a branching tree so forks and resumes work  
13. Compact session history when the context window fills up  
14. Emit an event stream that the UI subscribes to  
15. Missing piece from every agent's company building, I see. Carry one OpenTelemetry trace across every step so you can debug it

Every serious agent harnesses most of these. The expensive ones do all of them. The cheap ones cut corners and rebuild the corners later when they hit production. The frameworks bundle them into a monolith and ship one version of each. That last part is the part that costs you, because a year in, you find out that the policy engine you want is not the policy engine the framework ships, and replacing it means replacing the harness.

The iii harness ships every one of those thirteen jobs as a separate worker on the [workers.iii.dev](https://workers.iii.dev/) registry. Each speaks the same WebSocket protocol. Each registers functions and triggers on the same engine bus. Each is `iii worker add`\-able, swappable, and writable in any language with an SDK.

## **The stack, by worker**

Here is the actual production stack from the [iii-hq/workers](https://github.com/iii-hq/workers/tree/main/harness) monorepo, with each worker's job in one line. The whole bundle ships at [github.com/iii-hq/workers/harness](https://github.com/iii-hq/workers/tree/main/harness):

| Worker | Job |
| ----- | ----- |
| `iii-directory` | Skill and prompt registry. Workers publish skills at `iii://<worker>/<function>`; the agent fetches them on demand via `directory::skills::get`. Ships with the iii engine (Rust). |
| `harness` | Meta-worker. Loads `iii-permissions.yaml`. Exposes `policy::check_permissions` and the `ui::*` plane. Pumps `agent::events` to subscribed browsers. |
| `turn-orchestrator` | The durable 11-state FSM driving each agent turn. Owns `run::start`, `turn::step`, `turn::get_state`. Also assembles the system prompt at the `provisioning` step. |
| `approval-gate` | Bus entry point for operator decisions. Routes `approval::resolve` to per-call resume functions registered by the orchestrator. |
| `session` | Branching session storage. `session-tree::*` for the parent-linked entry tree; `session-inbox::*` for per-session queues. |
| `llm-budget` | Workspace \+ agent spend caps. 14 `budget::*` functions including check, record, alerts, forecast, period rollover. |
| `hook-fanout` | Generic publish-and-collect over a stream topic. The pattern every iii hook is built from. |
| `auth-credentials` | File-backed provider credential vault under `auth::*`. |
| `models-catalog` | Static model capability catalogue. `models::list`, `models::get`, `models::supports`. |
| `provider-anthropic` | Anthropic Messages API SSE streamed into an iii channel. |
| `provider-openai` | OpenAI Chat Completions SSE streamed into an iii channel. |
| `provider-kimi` | Kimi (Moonshot) Chat Completions SSE. |
| `provider-lmstudio` | Local LM Studio SSE for desktop development. |
| `context-compaction` | Optional `agent::events` that compacts session history when token count crosses a threshold. |

Eleven workers. One engine. Each is on a published version. Each is independently runnable as a standalone process (`pnpm dev:<worker>` in dev, `iii worker add <specific-worker>` as a release binary) or as part of the composite entry point that spins them up together.

The reason this matters: every box in that table is a place where someone can hand you a different worker, and you keep the rest. Don't like the static model catalogue? Plug in a worker that registers `models::list` and reads from a live API. Don't like file-backed credentials? Plug in a worker that registers `auth::get_token` and reads from a secrets manager. Want a different turn FSM for a workflow that branches differently? Replace `turn-orchestrator`, every dependent calls `run::start` and reads `turn_state` through the same bus, so the rest of the stack doesn't change.

## **How the loop actually runs**

The shape of one turn looks like this, walking through the workers in the order they fire.

A browser/cli/chat POSTs a turn through `harness::trigger` with `{session_id, message_id, payload}`. The harness meta-worker forwards `payload` to `run::start`. That hop exists so the OpenTelemetry span wrapper can seed the session and message IDs as baggage, which propagates to every nested `iii.trigger` call across every worker in the stack. The trace tree on the other side is one connected graph.

`run::start` lands on the turn-orchestrator. It persists the run request, seeds the initial `TurnStateRecord` in iii state at `session/<sid>/turn_state`, and returns immediately. The actual work happens inside the durable per-state machine, woken by publishes to the `turn-step` FIFO.

The two terminal states are `stopped` (clean exit via `finishSession()`) and `failed` (an unexpected handler throw routes here, acks the queue so it stops retrying, and surfaces `message_complete{stop_reason:'error'}` plus `agent_end` so the UI shows the reason). Teardown is an inline `finishSession()` port called from any turn-end path, not a separate enqueued step.

`provisioning` does three things. It boots a [iii-sandbox](https://github.com/iii-hq/iii/tree/main/crates/iii-worker/src/sandbox_daemon) microVM if the run needs isolated execution. It calls `directory::skills::download` for every namespace in `system_default_skills` (default `["iii://iii-directory/index"]`) so iii-directory pre-caches the skill bodies the run starts with. And it assembles the system prompt in three layers: a mode paragraph picked from `run_request.mode` (`plan`, `ask`, or `agent`), the iii identity preamble that teaches the model the `agent_trigger` convention and the `directory::skills::get` on-demand discovery pattern, and an appended index of the default skills the agent boots with. The caller can override the whole prompt by passing `system_prompt` on `run::start`; otherwise the orchestrator builds it. Function schemas come from the live engine catalog.

`assistant_streaming` calls `provider::<name>::stream` on whichever provider worker matches the run's `provider` field. The provider worker pulls credentials via `auth::get_token` (auth-credentials), streams the model's SSE response into an iii channel, and the orchestrator drains that channel emitting `message_update` events on `agent::events` for the UI fanout. Channel creation and the read loop live behind a pull-based `MessagePump` in `provider-stream.ts`, so the streaming state stays focused on transitions.

When the assistant returns tool calls, the FSM enters `function_execute`. Every tool call passes through `dispatchWithHook`, the single chokepoint in the orchestrator. `consultBefore` calls `policy::check_permissions` directly with a 5-second timeout. The policy worker (the harness meta-worker, in the default stack) reads `iii-permissions.yaml`, matches the call's `function_id` against the rule set, and returns one of three outcomes:

* `allow:` dispatch proceeds; the orchestrator triggers the target function and writes the result  
* `deny:` dispatch short-circuits with a DenialEnvelope, the result becomes a denial record  
* `needs_approval:` the individual call parks into the turn's awaiting\_approval list. The rest of the batch keeps dispatching. The turn transitions to function\_awaiting\_approval only when one or more entries are pending.

The approval wake is reactive and shared. The orchestrator registers exactly one `turn::on_approval` state trigger on scope `approvals`. When the console calls `approval::resolve`, the approval-gate worker writes `approvals/<sid>/<cid> = {decision, reason}` to iii state. That write fires `turn::on_approval`, which advances the affected session. `function_awaiting_approval` reads only the decisions that just landed, dispatches each one as it arrives (`allow` becomes a pre-approved dispatch, `deny` or `aborted` becomes a synthetic denial), and advances when `awaiting_approval[]` is empty. No per-call resume functions to register. No startup re-scan to recover pending approvals. One trigger covers every session.

Fail-closed by construction: if the policy worker is unreachable or the 5-second timeout fires, `consultBefore` denies the call with a `gate_unavailable` envelope. If `iii::durable::publish` itself errored, the hook fanout returns `publish_failed: true` and the orchestrator treats it as a deny.

A few latency wins fall out of this shape. The after-function-call hook short-circuits `publish_collect` via a subscriber-presence cache when no durable subscriber is registered for the topic, removing roughly 500ms per executed function call. `tearing_down` is inlined into `finishSession()`, removing one durable queue hop per turn. `context-compaction` subscribes to a dedicated `agent::turn_end` stream the orchestrator emits at turn boundaries, so compactor wakeups are per-turn instead of per-event. The session-create fanout state trigger gates by scope alone and matches in-process, so the previous per-write `harness::session::is_create_event` RPC is gone.

After the batch completes, `steering_check` decides whether to continue, stop, or hit `max_turns`. If continue, loop back to `assistant_streaming`. If stop or max, `finishSession()` runs inline: emit `agent_end`, free the sandbox, transition to `stopped`.

Throughout the whole run, every worker that participates emits OTel spans tagged with `iii.session.id`, `iii.message.id`, and `iii.function.id`. Those tags are what the engine's `engine::traces::group_by` reads to populate "Group by Session" / "Group by Message" / "Group by Function" in the traces UI. The instrumentation is automatic: `src/runtime/worker.ts` wraps every `registerFunction` in a `Proxy` so no per-worker code has to remember to add spans.

## **Build your own**

The interesting part is that none of the workers above are special. Each one is a process that opens a WebSocket to the engine, registers some functions and triggers, and runs. The contract is the same as the contract every application worker uses. The harness is built on the same primitive your business logic is built on.

Which means "build your own harness" decomposes into the same operation as "write any worker." You pick the layer you want to replace, you write a worker that registers the same functions on the bus, you `iii worker add` it, and the rest of the stack starts using your worker.

Two layers don't show up in the worker table above but matter for how the harness behaves. **Skills** are how each worker advertises what its functions do. Every worker can publish a skill at `iii://<worker>/<function>` that the agent fetches via `directory::skills::get` before calling that function for the first time. **The system prompt** is assembled per turn from a mode paragraph, the iii identity preamble, and the default skill bodies the run was configured with. Both are bus-driven: skills are served by the iii-directory worker, the system prompt is assembled by the turn-orchestrator. Both are replaceable.

Five concrete examples.

**Replace the model catalogue with a live API.** Write a worker that registers `models::list`, `models::get`, `models::supports`. Have it fetch from your provider's catalog endpoint every N minutes and cache. Publish it. `iii worker add your-org/dynamic-models-catalog`. Stop the static models-catalog worker. The turn-orchestrator never knows the difference. It calls `iii.trigger('models::list')` and the engine routes to whichever worker registered that function id most recently.

**Add a new provider.** The shape is `provider-kimi` and `provider-lmstudio` already prove out. Each is one worker that registers `provider::<name>::stream` and `provider::<name>::complete`, drains an SSE stream from the upstream API into an iii channel, and writes its model usage to llm-budget via `budget::record`. Adding a fifth provider is writing one folder with one `iii.worker.yaml` and one `register.ts`. Publish to the registry, or keep it local. The turn-orchestrator picks the provider by the run's `provider` field; new providers become available the instant the worker connects.

**Serve skills from a private artifact store.** Write a worker that registers `directory::skills::get` and `directory::skills::list`, backed by your internal docs system or a private S3 bucket. Disconnect or rename the default iii-directory worker. The orchestrator's bootstrap calls `directory::skills::download` per namespace; your worker answers. The agent's "fetch the per-function skill before calling a new function" pattern keeps working unchanged because the wire shape is the same.

**Override the system prompt entirely.** `run::start` accepts an optional `system_prompt` field. Pass it and the orchestrator uses your string verbatim, skipping the mode paragraph \+ identity preamble \+ skills appendix assembly. Useful when you have an existing prompt asset you want the harness to honour without modification. Skill download still runs in bootstrap, so the agent keeps `directory::skills::get` on-demand discovery even with a custom prompt.

**Replace the approval gate UI surface.** The default `approval-gate` worker registers `approval::resolve`. The wire schema is one function call:

```ts
iii.trigger('approval::resolve', {
  session_id: '...',
  function_call_id: '...',
  decision: 'allow' | 'deny' | 'aborted',
  reason: 'optional human text',
})
```

The handler persists `approvals/<sid>/<cid> = {decision, reason}` to iii state. The orchestrator's single `turn::on_approval` state trigger picks that write up and wakes the right session. If you want to drive approvals from Slack instead of the console, write a Slack worker that listens for `/approve <id>` and `/deny <id>` slash commands, then calls `approval::resolve` with the right payload. The orchestrator never knows the difference. The whole approval-gate worker stays untouched. You added a new worker; you didn't replace the existing one.

If you want a different policy engine (OPA, Cedar, your own DSL), write a worker that registers `policy::check_permissions` and returns `{ decision, rule_id?, matched_constraint? }`. Disconnect the default policy worker (which is wrapped inside the harness meta-worker, so you'd disable that handler or run a stripped-down meta-worker). The turn-orchestrator's `consultBefore` doesn't know the difference. Same 5-second timeout, same fail-closed semantics, same wire shape.

The point of these examples isn't the specific replacements. It's the shape of the operation. Every harness layer in the iii stack is reachable through one or two function ids on the bus. Replacing a layer is writing a worker that registers those ids. The rest of the system stays.

## **The harness is a slider, not a fork in the road**

The classic harness debate frames itself as thin vs thick. Anthropic's thin loop versus LangGraph's explicit DAG. The framing assumes you pick one side and live with it.

When the harness is composed of workers on the same bus, thin vs thick is just a count of how many workers you install. A thin harness is `turn-orchestrator` plus `provider-anthropic` plus `auth-credentials` plus a minimal `harness` meta-worker. That's it. No approvals, no budgets, no policy engine, no hook fanout. Run anything. Trust the model. Useful for autonomous research agents, experimental loops, anything internal.

A thick harness is all thirteen workers plus `context-compaction` plus a custom policy worker plus a custom approval-gate plus a Slack-integrated approval surface plus the budget worker enforcing per-workspace caps. Useful for an agent running customer workflows where every tool call needs to be auditable and every model spend has to roll up to a finance dashboard.

The architectural distance between thin and thick isn't a rewrite. It's a config change. Same primitives, same wire protocol, same trace shape, same observability story. The slider moves by adding and removing workers from your `config.yaml`. Everything else holds.

It applies inside a single worker too. The turn-orchestrator just shipped a refactor that collapsed its FSM from eleven states to seven, deleted the per-call `turn::approval_resume::<sid>/<cid>` mechanism in favour of one reactive `turn::on_approval` state trigger on scope `approvals`, and inlined `tearing_down` into a `finishSession()` port. Every other worker in the stack (approval-gate, session, llm-budget, providers, models-catalog, auth-credentials, hook-fanout, context-compaction) stayed unchanged. The `approval::resolve` wire shape didn't move. The contracts held. That's the property the composition gives you: a major internal rewrite of one worker is a self-contained change because every neighbour talks to it through bus-level function ids.

This is the part the framework model can't give you. A framework picks a position on the slider for you and locks you in. The worker model leaves the slider in your hand.

## **What this means in practice**

If you've been running an agent on top of a framework and feeling the same boundary problems most teams hit at scale, the answer is probably not "rewrite the harness in our own framework." The policy engine doesn't extend the way you need. The approval UI is wired into the framework's chat surface. The credential store can't talk to your secrets manager. The budget tracker is in a sidecar database the trace can't see. The answer is to switch to a substrate where the harness is decomposed in the first place.

The fastest way to feel the argument is to clone [github.com/iii-hq/workers](https://github.com/iii-hq/workers), `pnpm install`, `pnpm build`, and run the composite entry point. You'll get the full fourteen-worker harness pointed at an iii engine. You can disable any worker by removing its entry from the boot list. You can swap any worker by writing a replacement that registers the same function ids. You can extend any worker by adding a subscriber to its hook topics. `hook-fanout::publish_collect` is the generic every iii hook builds on.

The docs live at [iii.dev/docs](https://iii.dev/docs). The engine is at [github.com/iii-hq/iii](https://github.com/iii-hq/iii). The worker registry is at [workers.iii.dev](https://workers.iii.dev). The harness bundle is at [github.com/iii-hq/workers/harness](https://github.com/iii-hq/workers/tree/main/harness).

## **The bet**

A harness is not a thing you install. A harness is a set of jobs your system has to do for an agent to run durably, safely and observably. The framework era bundled those jobs together because nothing underneath gave you a way to compose them.

iii's bet is that one primitive: **a worker** that connects to the engine over WebSocket and registers functions and triggers is small enough to absorb every one of those jobs separately, and that the resulting stack is more useful than any framework because every layer is independently replaceable.

You don't adopt the iii harness. You install the workers you want, write the ones you need, and end up with a harness shaped exactly like your system. Same protocol on every layer. Same trace across every call. Same `iii worker add` for the parts you take from the registry as for the parts you publish yourself.

That's what "**build your own agent harness**" looks like when the substrate is the right shape. Pick the workers. Write the missing ones. Compose. The harness is the composition.

Join us in building the perfect agent harness that the modern world needs: [discord.gg/iiidev](http://discord.gg/iiidev)

iii is open source. Get started at [iii.dev/docs](https://iii.dev/docs). The harness workers are at [github.com/iii-hq/workers](https://github.com/iii-hq/workers) and the engine is at [github.com/iii-hq/iii](https://github.com/iii-hq/iii).

— Mike Piccolo, Founder & CEO

