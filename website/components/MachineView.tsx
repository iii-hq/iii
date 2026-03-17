import React, { useState } from "react";
import { Navbar } from "./Navbar";

interface MachineViewProps {
  onToggleMode: () => void;
  onToggleTheme: () => void;
  onOpenTerminal?: () => void;
  isGodMode: boolean;
  isDarkMode?: boolean;
  onLogoClick?: () => void;
}

export const MachineView: React.FC<MachineViewProps> = ({
  onToggleMode,
  onToggleTheme,
  isGodMode,
  isDarkMode = true,
  onLogoClick,
}) => {
  const [isLogoHovered, setIsLogoHovered] = useState(false);

  return (
    <div
      className={`min-h-screen font-mono relative flex flex-col transition-colors duration-300 ${
        isDarkMode
          ? "bg-iii-black text-iii-light"
          : "bg-iii-light text-iii-black"
      }`}
    >
      <Navbar
        isDarkMode={isDarkMode}
        isGodMode={isGodMode}
        isHumanMode={false}
        onToggleTheme={onToggleTheme}
        onToggleMode={onToggleMode}
        onLogoClick={onLogoClick}
        onLogoMouseEnter={() => setIsLogoHovered(true)}
        onLogoMouseLeave={() => setIsLogoHovered(false)}
      />

      <div className="flex-1 text-xs md:text-sm leading-relaxed px-4 md:px-8 lg:px-12 pt-24 md:pt-32 lg:pt-32 pb-8 overflow-x-hidden">
        <div className="max-w-4xl mx-auto space-y-4 md:space-y-6 break-words">
          <pre className="whitespace-pre-wrap break-words overflow-x-auto">{`iii — The Centralized Orchestration Runtime for Distributed Polyglot Function Execution

[Docs](https://iii.dev/docs) | [GitHub](https://github.com/iii-hq/iii)

## Install
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh`}</pre>

          <pre className="whitespace-pre-wrap break-words overflow-x-auto">{`# What is iii

One Engine. Three primitives: Function, Trigger, Worker.
React simplified frontend with Component and Context. iii does the same for backend.

| Primitive  | Role                                                        |
|------------|-------------------------------------------------------------|
| Function   | Anything that does work — receives input, returns output    |
| Trigger    | What makes a Function run — HTTP, cron, queue, state, stream|
| Worker     | Any process that registers functions and triggers            |

Key properties:
- Polyglot execution — any language participates through one universal protocol
- Complete observability — logs and traces auto-injected into every invocation
- Self-hosting / BYOC — connect existing domains and services, full portability
- Shared capabilities — State, Streaming, Observability accessible to every function

Languages: TypeScript, Python, Rust
Integrations: PostgreSQL, MongoDB, Redis, Kafka, GraphQL, gRPC
Platforms: AWS, Google Cloud, Azure, Cloudflare, Vercel, Fly.io, Docker, Kubernetes`}</pre>

          <pre className="whitespace-pre-wrap break-words overflow-x-auto">{`# SDK — One Protocol, Any Language

## TypeScript
\`\`\`typescript
import { registerWorker, Logger } from "iii-sdk"
const iii = registerWorker(process.env.III_BRIDGE_URL ?? "ws://localhost:49134")

iii.registerFunction(
  { id: "users::create" },
  async (input) => {
    const logger = new Logger()
    logger.info("Creating user", { email: input.email })
    return { id: "123", email: input.email }
  }
)

iii.registerTrigger({
  type: "http",
  function_id: "users::create",
  config: { api_path: "users", http_method: "POST" }
})
\`\`\`

## Python
\`\`\`python
from iii import register_worker, Logger

iii = register_worker(os.environ.get("III_BRIDGE_URL", "ws://localhost:49134"))

async def create_user(input):
    logger = Logger()
    logger.info("Creating user", { "email": input["email"] })
    return { "id": "123", "email": input["email"] }

iii.register_function("users::create", create_user)

iii.register_trigger(
    "http",
    "users::create",
    { "api_path": "users", "http_method": "POST" }
)
\`\`\`

## Rust
\`\`\`rust
use iii_sdk::{register_worker, InitOptions, Logger};
use serde_json::json;

let iii = register_worker("ws://localhost:49134", InitOptions::default())?;

iii.register_function("users::create", |input| async move {
    let logger = Logger::new();
    let email = input["email"].as_str().unwrap_or("");
    logger.info(&format!("Creating user: {}", email));
    Ok(json!({ "id": "123", "email": email }))
});

iii.register_trigger(Trigger {
    trigger_type: "http".into(),
    function_id: "users::create".into(),
    config: json!({ "api_path": "users", "http_method": "POST" }),
});
\`\`\`

## Core SDK Methods
- iii.registerFunction({ id }, handler) — register a function
- iii.registerTrigger({ type, function_id, config }) — bind a trigger
- iii.trigger({ function_id, payload }) — invoke a function (awaitable)
- iii.trigger({ function_id, payload, action: TriggerAction.Void() }) — fire-and-forget
- iii.listFunctions() — discover all available functions
- iii.onFunctionsAvailable(callback) — subscribe to topology changes
- new Logger() — structured logger with traceId correlation

## Built-in System Functions
- state::get / state::set — { scope, key, value }
- stream::set / stream::list — { stream_name, group_id, item_id, data }
- publish — { topic, data }
- enqueue — { topic, data }`}</pre>

          <pre className="whitespace-pre-wrap break-words overflow-x-auto">{`# Architecture — One Engine, Three Primitives

## Function — Anything that does work
A Function receives input and optionally returns output.
It can live anywhere — locally, on cloud, on serverless, or as a third-party HTTP endpoint.
- Write in TypeScript, Python, or Rust — mix freely
- Addressable by path (users.create, orders.process)
- Hot-swap handlers without restarting consumers
- Auto-cleanup when workers disconnect

## Trigger — What makes a Function run
A Trigger causes a Function to execute — either explicitly from code via trigger(),
or automatically from an event source like an HTTP request, cron schedule, queue message, or state change.
- HTTP, cron, queue, subscribe, state, stream triggers
- One function, many triggers — bind freely
- Custom trigger types plug in at runtime
- Same pattern for every event source

## Worker — Any process that registers functions
A Worker is any process that registers Functions and Triggers.
Long-running services, ephemeral scripts, agentic workers, or legacy systems via middleware.
- Workers register functions → immediately available to all
- Workers disconnect → functions removed, no stale refs
- Long-running, ephemeral, or agentic — all first-class
- Scale up, scale down — topology adapts in real time

## Engine Capabilities

| Capability        | Description              |
|-------------------|--------------------------|
| HTTP + Webhooks   | API triggers             |
| Cron + Schedules  | Timed execution          |
| Queues + Events   | Pub/Sub and topics       |
| State + Cache     | Shared context           |
| Streaming         | Realtime pipes           |
| Observability     | Logs + traces            |
| Workflows         | Multi-step orchestration |
| AI Agents         | Tool discovery           |

## Engine Properties
- Unified Invocation — same interface for local and remote functions
- Request-Response Correlation — sync-style triggers across async boundaries via invocation IDs
- Lifecycle Management — auto-cleanup of functions, triggers, invocations on disconnect
- Recursive Orchestration — engines can nest as workers of other engines`}</pre>

          <pre className="whitespace-pre-wrap break-words overflow-x-auto">{`# AI Agents — First-Class Citizens

The engine operates as a universal tool discovery and invocation layer where intelligent agents
participate as first-class execution entities — not an afterthought.

Compatible agents: Claude Code, Cursor, Gemini, Codex, Windsurf, Trae, Amp, Roo, Copilot, Cline, Goose

## 1. AI Agent with Tools — ReAct loop with tool calling
\`\`\`typescript
import { registerWorker, Logger, TriggerAction } from "iii-sdk"
const iii = registerWorker(process.env.III_BRIDGE_URL ?? "ws://localhost:49134")
const logger = new Logger()

const tools = await iii.listFunctions()

iii.registerFunction(
  { id: "agent::research" },
  async ({ query }) => {
    const response = await callLLM(query, { tools })
    while (response.toolCall) {
      const result = await iii.trigger({
        function_id: response.toolCall.function,
        payload: response.toolCall.args
      })
      logger.info("Tool used", { tool: response.toolCall.function })
      response = await callLLM(query, { tools, toolResult: result })
    }
    return response
  }
)
\`\`\`

## 2. Multi-Agent Network — Researcher → Analyzer → Writer pipeline
\`\`\`typescript
iii.registerFunction({ id: "agents::researcher" }, async ({ topic }) => {
  const sources = await iii.trigger({ function_id: "tools::webSearch", payload: { query: topic } })
  return iii.trigger({ function_id: "agents::analyzer", payload: { sources, topic } })
})

iii.registerFunction({ id: "agents::analyzer" }, async ({ sources, topic }) => {
  const insights = await callLLM("Analyze these sources", { sources })
  return iii.trigger({ function_id: "agents::writer", payload: { insights, topic } })
})

iii.registerFunction({ id: "agents::writer" }, async ({ insights, topic }) => {
  const draft = await callLLM("Write a report", { insights })
  await iii.trigger({ function_id: "state::set", payload: {
    scope: "reports", key: topic, value: draft
  } })
  iii.trigger({ function_id: "publish", payload: { topic: "report.ready", data: { topic } }, action: TriggerAction.Void() })
  return draft
})
\`\`\`

## 3. Durable Workflows — Checkpoint/resume patterns
\`\`\`typescript
iii.registerFunction({ id: "orders::process" }, async ({ orderId }) => {
  const logger = new Logger()
  const step = await iii.trigger({ function_id: "state::get", payload: {
    scope: orderId, key: "step"
  } }) ?? 0

  const pipeline = [
    () => iii.trigger({ function_id: "payments::charge", payload: { orderId } }),
    () => iii.trigger({ function_id: "inventory::reserve", payload: { orderId } }),
    () => iii.trigger({ function_id: "shipping::create", payload: { orderId } }),
    () => iii.trigger({ function_id: "notifications::send", payload: { orderId } }),
  ]

  for (let i = step; i < pipeline.length; i++) {
    await pipeline[i]()
    await iii.trigger({ function_id: "state::set", payload: {
      scope: orderId, key: "step", value: i + 1
    } })
    logger.info("Step completed", { orderId, step: i + 1 })
  }
  return { status: "completed" }
})
\`\`\`

## 4. Polyglot Workers — TS + Python + Rust as one system
\`\`\`typescript
iii.registerFunction({ id: "api::users" }, async (req) => {
  const user = await db.createUser(req)
  iii.trigger({ function_id: "publish", payload: { topic: "user.created", data: user }, action: TriggerAction.Void() })
  return user
})

iii.registerTrigger({
  type: "http", function_id: "api::users",
  config: { api_path: "users", http_method: "POST" }
})

iii.registerTrigger({
  type: "subscribe", function_id: "ml::onboarding",
  config: { topic: "user.created" }
})
\`\`\`

## 5. Real-Time Streaming — Chat with auto-summarization
\`\`\`typescript
iii.registerFunction({ id: "chat::send" }, async ({ roomId, message }) => {
  const logger = new Logger()
  await iii.trigger({ function_id: "stream::set", payload: {
    stream_name: "chat", group_id: roomId,
    item_id: crypto.randomUUID(), data: message
  } })
  const history = await iii.trigger({ function_id: "stream::list", payload: {
    stream_name: "chat", group_id: roomId
  } })
  if (history.length > 100) {
    const summary = await iii.trigger({ function_id: "agents::summarize", payload: { history } })
    await iii.trigger({ function_id: "state::set", payload: {
      scope: roomId, key: "summary", value: summary
    } })
  }
  logger.info("Message sent", { roomId, messages: history.length })
})
\`\`\`

## 6. Deep Research Agent — Iterative multi-step research with memory
\`\`\`typescript
iii.registerFunction({ id: "research::deep" }, async ({ question, depth = 3 }) => {
  const logger = new Logger()
  let context: string[] = []
  for (let i = 0; i < depth; i++) {
    const subQueries = await callLLM("Break into sub-questions", { question, context })
    const results = await Promise.all(
      subQueries.map((q: string) => iii.trigger({ function_id: "tools::webSearch", payload: { query: q } }))
    )
    context.push(...results.flat())
    const assessment = await callLLM("Is this enough?", { question, context })
    if (assessment.sufficient) break
    logger.info("Research iteration", { iteration: i + 1, sources: context.length })
  }
  const report = await callLLM("Write comprehensive answer", { question, context })
  await iii.trigger({ function_id: "state::set", payload: { scope: "research", key: question, value: report } })
  return report
})
\`\`\`

## 7. Event-Driven Pipelines — user.created → parallel CRM + analytics + ML + email
\`\`\`typescript
iii.registerFunction({ id: "pipeline::onUserCreated" }, async ({ user }) => {
  const logger = new Logger()
  await Promise.all([
    iii.trigger({ function_id: "crm::syncContact", payload: { user } }),
    iii.trigger({ function_id: "analytics::track", payload: { event: "signup", user } }),
    iii.trigger({ function_id: "ml::computeSegment", payload: { user } }),
  ])
  const segment = await iii.trigger({ function_id: "state::get", payload: { scope: user.id, key: "segment" } })
  await iii.trigger({
    function_id: "emails",
    payload: { template: segment === "enterprise" ? "white-glove" : "welcome", user },
    action: TriggerAction.Enqueue({ queue: "emails" })
  })
  logger.info("Pipeline complete", { userId: user.id, segment })
})

iii.registerTrigger({
  type: "subscribe", function_id: "pipeline::onUserCreated",
  config: { topic: "user.created" }
})
\`\`\`

## 8. Scheduled Intelligence — Cron + AI anomaly detection
\`\`\`typescript
iii.registerFunction({ id: "monitor::anomalies" }, async () => {
  const logger = new Logger()
  const metrics = await iii.trigger({ function_id: "metrics::getLast24h", payload: {} })
  const baseline = await iii.trigger({ function_id: "state::get", payload: {
    scope: "monitor", key: "baseline"
  } })
  const analysis = await callLLM(
    "Analyze metrics against baseline. Flag anomalies.", { metrics, baseline }
  )
  if (analysis.anomalies.length > 0) {
    await iii.trigger({ function_id: "alerts::send", payload: {
      channel: "slack", message: analysis.summary,
      severity: analysis.anomalies[0].severity
    } })
    logger.info("Anomalies detected", { count: analysis.anomalies.length })
  }
  await iii.trigger({ function_id: "state::set", payload: {
    scope: "monitor", key: "baseline",
    value: { ...baseline, ...metrics.averages }
  } })
})

iii.registerTrigger({
  type: "cron", function_id: "monitor::anomalies",
  config: { pattern: "*/15 * * * *" }
})
\`\`\``}</pre>

          <pre className="whitespace-pre-wrap break-words overflow-x-auto">{`# What iii Replaces — 50+ Tools, Three Primitives

| Category        | Traditional Tools                         | iii Primitive             |
|-----------------|-------------------------------------------|--------------------------|
| API Frameworks  | Express, Flask, FastAPI, Koa, Hono        | Function + Trigger (http)|
| Background Jobs | Bull, Celery, Sidekiq, Agenda, Dramatiq   | Function + async trigger |
| Message Queues  | Redis Pub/Sub, RabbitMQ, Kafka, NATS      | Function + Trigger (event)|
| Real-time       | Socket.io, Pusher, Ably, Liveblocks       | Streaming                |
| State & Cache   | Redis, Memcached, DynamoDB                | State                    |
| Scheduled Tasks | node-cron, Agenda, Cloud Scheduler        | Trigger (cron)           |
| Observability   | Winston, Pino, OpenTelemetry, Datadog SDK | Built-in tracing         |
| Workflows       | Temporal, Cadence, Step Functions          | State + Events           |

## Platforms it enables

| Platform              | iii Pattern                                                  |
|-----------------------|--------------------------------------------------------------|
| AI Agent Runtime      | Functions = Tools, State = Memory, Streams = Responses       |
| Feature Flag System   | State + Streams = Real-time Toggles                          |
| Multiplayer Games     | Streams = Game State, Events = Actions                       |
| ETL Pipelines         | Events = Data Flow, State = Checkpoints                      |
| Reactive Backend      | Triggers + Events + State                                    |`}</pre>

          <pre className="whitespace-pre-wrap break-words overflow-x-auto">{`# Built-in Capabilities — Zero Configuration

Start building now. Scale your way.
Built-in defaults get you running without thinking about architecture.
When you outgrow them, swap in Redis, BullMQ, or whatever you prefer.
All capabilities available via the same iii protocol — accessible from any language.

## Shared State — Cross-language state access
Python worker sets a value, Node.js worker reads it instantly — no Redis required.
  iii.trigger({ function_id: "state::set", payload: { scope: "user:123", key: "prefs", value: data } })
  iii.trigger({ function_id: "state::get", payload: { scope: "user:123", key: "prefs" } })

## Real-time Streaming — Bidirectional data flows
Stream data between workers in real-time. Process infinite sequences without buffering.
  iii.trigger({ function_id: "stream::set", payload: { stream_name: "feed", group_id: id, item_id: uuid, data: chunk } })
  iii.trigger({ function_id: "stream::list", payload: { stream_name: "feed", group_id: id } })

## Complete Observability — Auto-injected tracing
Every invocation carries a trace ID. Logs and metrics flow automatically.
  const logger = new Logger()
  logger.info("Processing", { orderId })

## Event Bus — Pub/sub between workers
Publish events from any worker, subscribe from any other.
  iii.trigger({ function_id: "publish", payload: { topic: "order.created", data: order }, action: TriggerAction.Void() })
  iii.registerTrigger({ type: "subscribe", function_id: "notify", config: { topic: "order.created" } })`}</pre>

          <pre className="whitespace-pre-wrap break-words overflow-x-auto">{`# FAQ

## How is iii different from gRPC?
gRPC needs compile-time IDL and codegen. iii uses runtime registration — functions available the moment a worker connects.

## How is iii different from a service mesh?
Service meshes need sidecars and complex networking. iii is one binary — workers connect via WebSocket, nothing else.

## Can I use iii with my existing Express/Flask/Spring app?
Yes. Add the SDK, register routes as functions. They join the distributed architecture instantly. Incremental adoption.

## What about AI agents and LLMs?
Functions self-describe with schemas. Agents discover and trigger them autonomously. Everything is auto-generated.

## Is iii production-ready?
Active development. Join Discord for early access and to shape what ships next.`}</pre>

          <pre className="whitespace-pre-wrap break-words overflow-x-auto">{`# Manifesto — 10 Paradigm Shifts

The future of backend engineering demands a new foundation. Not another framework. Not another protocol.
A universal execution kernel built on primitives that compose infinitely.

 1. POLYGLOT — Language agnostic by design. Node.js, Python, Rust, Go, browser, edge, embedded — all connect via the same protocol.
 2. SMALL SURFACE AREA — A small set of core primitives that compose infinitely. One kernel replaces domain-specific frameworks.
 3. UNIVERSAL ACCESSIBILITY — Every dependency and integration accessible to every service. Legacy servers, edge functions, serverless, embedded devices — all first-class participants.
 4. SECURE BIDIRECTIONAL COMMUNICATION — Every service can push and pull. Every connection is encrypted. Trust is built into the protocol.
 5. DYNAMIC REGISTRATION — Workers connect, register functions, and they're immediately available. No compilation, no code generation, no spec files.
 6. SELF DISCOVERABLE — The mesh knows what exists and how to reach it. No external discovery layer required.
 7. OBSERVABLE BY DEFAULT — Tracing, metrics, and logging built into the protocol. Every invocation is observable. Every transaction is traceable.
 8. POLYMORPHIC TRIGGERS — HTTP, state updates, gRPC, cron, events, hardware interrupts — all normalize to the same invocation model. One function, infinite triggers.
 9. AGENT-FIRST — Maximize the surface area for agent success. Functions self-describe with schemas and semantic metadata. The system adapts to agent behavior, not vice versa.
10. REVERSIBLE TRANSACTIONS — Every transaction chain is replayable, modifiable, and reversible. Debug by replaying. Recover by rewinding.

One Binary. Infinite Systems.`}</pre>

          <pre className="whitespace-pre-wrap break-words overflow-x-auto">{`# Resources

[Documentation](https://iii.dev/docs)
[GitHub](https://github.com/iii-hq/iii)
[npm](https://npmjs.com/package/iii-sdk)
[Discord](https://discord.gg/motia)

Install: curl -fsSL https://install.iii.dev/iii/main/install.sh | sh`}</pre>

          <pre className="whitespace-pre-wrap break-words overflow-x-auto text-gray-500">{`---
Motia LLC — Interoperable Invocation Interface`}</pre>
        </div>
      </div>
    </div>
  );
};
