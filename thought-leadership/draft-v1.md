# The Harness Is the Backend

The most important architectural question in AI infrastructure right now isn't which model to use. It's how much infrastructure to wrap around it.

Anthropic, OpenAI, CrewAI, LangChain — they each call that wrapping the *harness*: the orchestration loop, tools, memory, context management, and error handling that make a stateless model useful. They all agree the model isn't the product. The infrastructure is. They disagree — deeply — on how much of it should exist.

Anthropic keeps theirs thin. A loop. Assemble the prompt, call the model, execute tool calls, repeat. The model decides everything. OpenAI adds more structure: instruction stacks, orchestration modes, explicit handoff patterns. CrewAI splits the world: deterministic Flows for routing and validation, autonomous Crews for the rest. LangGraph goes furthest — every decision is a node, every transition a defined edge, the entire workflow encoded in the harness.

The spectrum runs from *trust the model* to *encode the logic*. And every team building agents has to place their bet somewhere on it.

But there's an assumption buried in the debate that nobody is questioning: that the harness and the backend are different things.

That cognitive orchestration — the agent's loop, its tools, its memory — lives in one layer. And execution infrastructure — queues, state, HTTP routing, cron, observability — lives in another. Two systems. One seam between them.

What if that seam is the actual problem?

---

## Two systems, one seam

Here's how most agent architectures work. The harness is a Python process (or TypeScript, or a managed framework) that wraps the model. When the agent decides to act, the harness translates a tool call into an HTTP request, a queue publish, a database write — some interaction with "the backend." The backend is its own world: separate services, separate config, separate failure modes.

The harness retries on its own schedule. The queue retries on its own schedule. The HTTP layer times out on a third schedule. None of them share a trace. When something breaks across the seam, debugging means correlating logs across systems that were never designed to talk to each other.

This gets worse with every agent you add. Worse with every language boundary. Worse with every team that owns a different piece of the stack. The industry treats this as two parallel problems — make the harness better, make the backend better — and hopes the seam holds.

But the seam is load-bearing. And it keeps cracking because we keep building two systems and asking them to cooperate.

---

## Three primitives, one system

iii is a backend engine built on three primitives:

A **Function** is a unit of work with a stable identifier — `orders::validate`, `agents::researcher`, `llm-service::respond` — that receives input, optionally returns output, and can live in any process, in any language. Functions invoke other functions via `trigger()`. The engine handles routing, serialization, and delivery.

A **Trigger** is what causes a function to run. You register a trigger to bind any event source to any function — an HTTP endpoint, a cron schedule, a queue subscription, a state change, a stream event, or any custom trigger type. Triggers are declarative: the worker says "this function runs when this thing happens," and the engine handles the binding.

A **Worker** is any process that connects to the engine and registers functions and triggers.

A TypeScript API service is a worker. A Python ML pipeline is a worker. A Rust microservice is a worker. And an agent is a worker.

This is the idea that changes everything. An agent connects to the engine, registers functions and triggers, persists context through `state::set`, hands off work through queue-backed triggers, and broadcasts results via pub/sub. It doesn't call "the backend" through a separate integration layer. It participates in the same system, with the same primitives, as everything else.

```
const iii = registerWorker('ws://localhost:49134', { workerName: 'agentic-backend' })

// A function: the unit of work
iii.registerFunction('agents::researcher', async (data) => {
  const findings = await research(data.topic)

  await iii.trigger({
    function_id: 'state::set',
    payload: { scope: 'research-tasks', key: data.task_id, value: findings }
  })

  iii.trigger({
    function_id: 'agents::critic',
    payload: { task_id: data.task_id },
    action: TriggerAction.Enqueue({ queue: 'agent-tasks' })
  })

  return findings
})

// Triggers: what causes the function to run
iii.registerTrigger({
  type: 'http',
  function_id: 'agents::researcher',
  config: { api_path: '/agents/research', http_method: 'POST' }
})

iii.registerTrigger({
  type: 'state',
  function_id: 'agents::researcher',
  config: { scope: 'research-tasks', condition: 'status == "pending"' }
})
```

Three calls. `registerFunction` defines the work. `registerTrigger` binds it to the world — in this case an HTTP endpoint *and* a state change reaction, for the same function. The researcher is now callable via a POST request and automatically fires whenever a research task enters a pending state. Add another trigger and it also runs on a cron schedule. The function doesn't change. The triggers compose.

The agent stores state with the same `trigger()` call a payment service would use. It hands off to the critic through the same queue mechanism an order pipeline would use. The agent's "tools" are functions. Its "memory" is state. Its "orchestration" is triggers and composition. There is no special agent infrastructure because there doesn't need to be.

The harness *is* the backend.

---

## Workers all the way down

This goes deeper than agents fitting into a backend. It's about what iii considers a primitive — and what happens when one primitive is the answer to every question.

In most platforms, every new capability is a new category. Need queues? Evaluate queue products. Need streaming? Different product. Sandboxing? Another. Each has its own internals, its own lifecycle, its own integration story. The platform is a catalog. Your job is to shop it and wire it together.

In iii, the answer to almost any question is the same: **add a worker, which in turn registers triggers and functions.**

I want sandboxing. Add a worker. I want an agent that researches topics. Add a worker. I want real-time streaming. Add a worker. I want go-to-market capabilities — lead scoring, email sequences, CRM sync. Add a worker. I want cron scheduling. It's already a worker. I want observability. Already a worker.

The worker connects, registers what it can do, and the system absorbs it — live, discoverable, observable. The answer doesn't change based on what kind of capability you're adding. It doesn't change based on language, or whether it's infrastructure or business logic, or whether a human or an agent is creating it. Add a worker.

This is not just architectural uniformity. It's a collapse of categories. In traditional systems, every capability lives in its own ontology — queues have broker semantics, HTTP has routing semantics, cron has scheduling semantics, agents have orchestration semantics. In iii, they are all the same thing: a process that registers functions and triggers. The semantics live in the functions, not in the infrastructure.

Paradigm shifts in software don't add features. They collapse categories. "Everything is a file" made Unix composable. Components as functions made React's mental model stick. In iii, the answer is always "add a worker." That's the primitive. That's the whole model.

---

## A live system

Because everything is a worker, the engine produces three properties that traditional architectures cannot:

**Live discovery.** When a worker connects, it receives the full catalog of every function registered across every other worker. When new functions appear, every worker gets a push notification. When a worker disconnects, its functions vanish and everyone is notified. The engine *is* the registry — no Consul, no DNS propagation, no stale OpenAPI specs.

For agents, this is cognitive infrastructure. An agent can see what the system can do *right now* — not what a spec said three deployments ago. Deploy a new specialist agent — `agents::legal-review` — and every other agent and service in the system immediately knows it exists. The function catalog *is* the tool registry.

**Live extensibility.** Add new workers and capabilities to a running iii system without redeploying or redesigning the architecture. A new Python worker appears, registers new functions, and the entire system — agents included — can invoke them instantly. No config changes. No restarting existing services. The system extends at runtime.

This is how agent systems actually need to grow. You don't shut down production to add a new capability. You connect a new worker. Its functions light up across the system. Agents that can use them, do.

**Live observability.** iii's observability is built on OpenTelemetry. Every function invocation carries a trace ID. Every `trigger()` call propagates it — across workers, across languages, across queue handoffs. Every log emitted through the iii Logger is automatically correlated to the active trace and span, emitted as structured OpenTelemetry LogRecords, and routed to whichever backend you use — the iii Console, Grafana, Jaeger, Datadog. This isn't bolted-on instrumentation. It's the wire protocol. Traces, metrics, and structured logs are produced by the engine itself, not by application-level middleware you have to remember to install.

When an agent calls a tool that enqueues a message that triggers a downstream function that writes state, the entire chain is one trace. Not three systems stitched together with timestamp correlation. One trace, across languages, across workers, across the agent-backend boundary that no longer exists. You go from a slow waterfall span directly to the correlated logs that explain what happened — without grepping stdout or cross-referencing timestamps.

---

## Agents that create workers

Here is where the model gets truly recursive.

iii supports sandbox workers — hardware-isolated microVMs, each with its own filesystem, network stack, and process tree. You create one with a single command: `iii worker add ./my-project`. The engine manages the VM lifecycle. The sandbox worker connects to the engine, registers functions and triggers, and participates in the system exactly like every other worker.

Now consider what happens when an agent can do this.

An agent worker — itself just a set of registered functions — can spin up a new sandbox worker at runtime. That sandbox gets its own isolated environment. It registers its own functions and triggers. Those functions immediately appear in the live catalog. Other agents and services can invoke them. When the sandbox is no longer needed, it disconnects and its functions vanish.

The sandbox is not a separate "sandbox product." It is a worker — the same primitive as everything else — that happens to provide hardware isolation. An agent creating a sandbox worker is just one worker creating another. The composition model doesn't change.

This is what it looks like when infrastructure becomes a design pattern instead of a product category. Need isolated execution for untrusted code? That's a sandbox worker. Need a temporary specialist agent? Spin up a worker, register functions, tear it down when finished. Need a fleet of parallel task executors? Workers. The primitive is the same. The pattern varies.

---

## The distinction disappears

Go back to the harness debate. Anthropic says thin. LangGraph says thick. They're arguing about how much cognitive structure to encode around the model. That's a real question. But it's a question *within* a design space, not about the design space itself.

When agents are workers, thin versus thick is just a question of how many functions you register and how you compose them. A thin harness is an agent worker with a few functions that lets the model decide what to `trigger()` next. A thick harness is an agent worker with more functions, explicit approval gates, conditional logic before enqueuing the next step. Same primitives. Same system. Different pattern.

The scaffolding metaphor shifts too. The industry talks about harness scaffolding as temporary — as models improve, you remove it. Manus rebuilt their agent five times in six months, each rewrite removing complexity. Anthropic strips planning steps from Claude Code whenever a new model absorbs the capability.

If the harness is built from the same primitives as the rest of the backend, then removing scaffolding just means simplifying a function. You don't rearchitect an integration layer. You don't rebuild the seam between two systems. You just register fewer functions, or compose them differently. The agent was never on top of the backend. It was always inside it.

---

## Anything is a worker

A worker is anything that can open a WebSocket and speak the primitives interface. There is no constraint on what that thing is or what language it's written in.

iii ships SDKs for TypeScript, Python, and Rust. But those aren't the boundary of the system — they're three implementations of an open wire protocol: JSON over WebSocket. The engine doesn't know what language is on the other end of the connection. It doesn't care. It sees functions, triggers, and a connection. If your team writes Go, or Java, or Swift, or Zig — you write an SDK that speaks the protocol and you're a first-class participant. The primitives interface is the contract. Everything else is a choice.

This means the set of what can be a worker is genuinely unbounded. A Node.js service. A Python ML pipeline. An agent. A queue. A sandbox running inside a microVM. A browser — iii ships a browser SDK, so a tab on someone's laptop can register functions, participate in live discovery, invoke backend functions, and be invoked *by* backend functions. The browser is in the system the same way a Kubernetes pod is.

A Raspberry Pi is a worker. An IoT sensor at the edge is a worker. A phone running a thin client is a worker. A CI runner that spins up, registers a function, does work, and disconnects is a worker. The engine doesn't distinguish between these. Every new language, every new device, every new runtime that implements the primitives interface gets the full system for free: live discovery, live extensibility, live observability, durable triggers, cross-everything invocation. Not because iii built a special integration for each one, but because the primitive doesn't discriminate.

---

## The bet

The industry is debating how much scaffolding to wrap around the model. That debate matters. But it takes for granted that the harness is its own world — separate from the backend, separate from the infrastructure that actually runs when a tool fires.

iii makes a different bet. That the right primitives — function, trigger, worker — are small enough and universal enough that the question "what can participate in this system?" has the answer: anything. A cloud service. An agent. A browser. A microcontroller. A sandbox an agent just spun up. They all compose the same way. They all discover each other. They all trace the same.

When you stop treating "agent infrastructure" as separate from "backend infrastructure" — when you stop treating *any* category of participant as architecturally different from any other — the system simplifies in a way that adding features never achieves. The boundaries between harness and backend, between cloud and edge, between infrastructure and application, between human-written services and agent-created workers — they all dissolve into the same three primitives.

The harness isn't on top of the backend. The harness *is* the backend. And the backend is whatever connects.

When you get the primitives right, the categories collapse. That's how paradigm shifts happen.

---

*[iii](https://iii.dev) is open source. [Get started](https://iii.dev/docs/quickstart), read the [architecture docs](https://iii.dev/docs/advanced/architecture), or give your AI coding agent full context with `npx skills add iii-hq/iii`.*
