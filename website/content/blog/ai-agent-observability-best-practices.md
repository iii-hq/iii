---
title: '8 AI agent observability best practices for tracing tool execution'
seoTitle: '8 AI Agent Observability Best Practices for Tool Calls'
description: 'Learn 8 AI agent observability best practices for tracing tool calls, tracking latency and token usage, and protecting sensitive data, with examples in iii.'
pubDate: 2026-11-23
tags: ['agents', 'observability', 'tracing', 'opentelemetry', 'harness']
ogImage: ../../assets/blog/ai-agent-observability-best-practices/og.png
faq:
  - q: 'What are traces in AI agents?'
    a: 'A trace is a tree of timed spans that records the instrumented steps in an agent execution, such as model calls, tool calls, and handoffs. The spans share a trace_id. The turn sits at the root, model and tool calls nest below it, and outbound service calls nest below those. Each span has a start time, an end time, attributes, and optionally span events for richer data like function inputs and outputs.'
  - q: 'How does agent observability differ from LLM monitoring?'
    a: 'LLM monitoring watches model calls: latency, token counts, and errors at the inference boundary. Agent observability follows the whole run, including every tool the agent called, the downstream services those tools reached, and the handoffs between workers. OpenTelemetry''s GenAI agent-span conventions use a similar hierarchy: an invoke_agent span is intended to be the parent of the agent''s tool and task spans.'
  - q: 'Can an agent read its own traces?'
    a: 'In iii, yes. The engine exposes engine::traces::list and engine::traces::tree, which a worker can call to retrieve trace data for the current or past sessions. The AI Agent Harness only allows calls to functions it has been explicitly authorized to reach, so you can deliberately grant an agent access to trace functions. Grant read-only access only when the agent needs it, with the same scrutiny you would apply to other operational data.'
---

![Tracing agent tool calls: an agent turn with a model call and a tool call nested beneath it.](../../assets/blog/ai-agent-observability-best-practices/banner.png)

Your agent says it added a label to a GitHub issue, but the label isn't there. You go into the logs to debug.

The logs show a request and a response, but there's no shared ID to connect them. You can't tell when the function started or finished, or whether it succeeded or hid an upstream error. The data is there, but the logs don't show how the steps fit together.

Agent observability connects those steps so you can follow a run from start to finish. This guide covers what to record on tool calls, how to set up tracing, and 8 practices that make traces easier to use. The walkthrough shows what end-to-end tracing looks like in iii.

> **Versions:** the worked example runs through a TypeScript worker that calls the GitHub REST API using iii engine 0.25.0, AI Agent Harness 1.8.61, and the ade console 1.9.70, verified October 22 to 23, 2026. iii traces SDK-handled worker calls and supports HTTP and queue handoffs in its console without any instrumentation code.

## What to record for every tool call

Give each call an ID that travels with it and nest it under the agent turn that ran it. Record its outcome, timing, and the step's token use. To connect a model's tool call to its result, pair the model's tool call ID with your trace and span IDs, and keep the result on the same span as the call.

The [OpenTelemetry GenAI conventions](https://opentelemetry.io/docs/specs/semconv/gen-ai/) define names for commonly recorded fields. They have "Development" status as of October 23, 2026, so those names may change.

| Field | OpenTelemetry name (Development status) | Where iii records it |
| --- | --- | --- |
| Trace and span ID | `trace_id`, `span_id` (W3C `traceparent`) | Every span, automatically |
| Run and turn ID | `gen_ai.conversation.id` | `iii.session.id` and `iii.message.id`, propagated via baggage onto recording spans |
| Tool call ID | `gen_ai.tool.call.id` | Not recorded as a span attribute. The provider's raw call ID (for example, Anthropic's `tool_use.id`) is preserved in the `FunctionCall` and `FunctionResult` content blocks of the session transcript (`session::messages`), which pair each call with its result. An execute span is a sibling of `router::chat` under the same turn step. Its position does not carry the call ID or distinguish calls when a turn requests multiple tools. |
| Tool name | `gen_ai.tool.name` | Span name: `execute` plus the function ID |
| Arguments and result | `gen_ai.tool.call.arguments`, `gen_ai.tool.call.result` (opt-in in the OTel spec) | `iii.invocation.input` and `iii.invocation.output` span events, recorded by default |
| Outcome and error | Span status, `error.type` | Span status, `iii.payload.ok`, `iii.turn.status` |
| Timing | Span start and end time | Every span, visible in the console waterfall |
| Token usage | `gen_ai.usage.input_tokens`, `gen_ai.usage.output_tokens`, `gen_ai.usage.cache_read.input_tokens`, `gen_ai.usage.cache_write.input_tokens` | `gen_ai.usage.*` on `execute router::chat`. Input and output tokens use the spec's names. Cache and reasoning counts (`gen_ai.usage.cache_read_tokens`, `cache_write_tokens`, `reasoning_tokens`) are iii-specific extensions under the `gen_ai` namespace, not the spec's field names. Input is normalized to uncached usage. |

## What you install before you see your first agent trace

To inspect a tool call, you need a way to generate, store, and view its spans. The table below compares what each stack requires, checked October 23, 2026.

| Stack | What you install, run, and add | Does it capture payloads by default? |
| --- | --- | --- |
| iii | [Install iii](https://iii.dev/docs/install). Tracing is included and the `iii-observability` worker is injected automatically. **Then:** Nothing for the console. Set `exporter: both` and add an endpoint to export traces to an external collector. | Yes: function inputs and outputs are recorded as span events. Secret-like keys (containing `api_key`, `password`, `secret`, `token` and similar patterns) are masked automatically. |
| Plain OpenTelemetry | OTel SDK, OTLP exporter, an instrumentation library such as [OpenLLMetry](https://github.com/traceloop/openllmetry) or OpenInference, and a backend such as Jaeger. A Collector is optional but recommended for production. **Then:** SDK initialization at startup. Add manual spans around your own tool functions. | Library-dependent. OpenLLMetry captures content by default. The GenAI conventions mark `gen_ai.tool.call.arguments` and `gen_ai.tool.call.result` as opt-in. |
| LangSmith | `langsmith` package and a hosted or self-hosted service. **Then:** `LANGSMITH_TRACING=true`, an API key, `@traceable` decorator on tool functions. | Yes, by default; opt out with `LANGSMITH_HIDE_INPUTS` or `LANGSMITH_HIDE_OUTPUTS`, or mask selectively with a rule-based anonymizer. See [LangSmith's masking docs](https://docs.langchain.com/langsmith/mask-inputs-outputs). |
| Langfuse (self-hosted) | `langfuse` SDK, Docker Compose stack of web, worker, Postgres, ClickHouse, Redis, and MinIO. **Then:** Keys and base URL in environment variables, plus decorators or an OTel bridge. | Yes, by default; masking is opt-in, via a custom mask function passed to the SDK or span processor. See [Langfuse's masking docs](https://langfuse.com/docs/observability/features/masking). |
| Arize Phoenix | `arize-phoenix-otel`, OpenInference instrumentors, and the Phoenix server. **Then:** `register(auto_instrument=True)` at startup. | Yes, by default; opt out per field with `OPENINFERENCE_HIDE_INPUTS` and `OPENINFERENCE_HIDE_OUTPUTS` (both default `false`). See [Phoenix's masking docs](https://arize.com/docs/phoenix/tracing/how-to-tracing/advanced/masking-span-attributes). |
| OpenAI Agents SDK | Nothing extra. **Then:** OpenAI API key for its hosted dashboard. A custom trace processor to send data elsewhere. Hosted tracing is unavailable under ZDR. | Yes: `trace_include_sensitive_data` defaults to `true`. See [OpenAI Agents SDK tracing docs](https://developers.openai.com/api/docs/guides/agents/integrations-observability#tracing). |
| Claude Code | An OTLP backend of your own. **Then:** `CLAUDE_CODE_ENABLE_TELEMETRY=1` alone only turns on metrics and logs. Tool-call spans require a separate beta flag: add `CLAUDE_CODE_ENHANCED_TELEMETRY_BETA=1` plus `OTEL_TRACES_EXPORTER` and OTLP endpoint settings. | No: prompts and tool content are redacted by default. Capturing them requires another opt-in flag after tracing is enabled. See [Claude Code monitoring docs](https://code.claude.com/docs/en/monitoring-usage). |

LangSmith, Langfuse, Arize Phoenix, Jaeger, Datadog, and other OTel-compatible services are all valid export targets for iii. The `exporter: both` setting keeps traces visible in the iii console while sending them to any of these backends.

## 8 best practices for tracing tool execution

### 1. Give every run, turn, and tool call an ID

Without shared IDs, log lines from the same agent run are hard to connect. Trace and span IDs connect steps across process boundaries. A run or conversation ID lets you find the traces for a job, user session, or failing ticket without relying on timestamps.

The model's tool call ID identifies a specific tool invocation. It differs from the span ID your framework creates. Keep both alongside the run or conversation ID so you can connect the model's request to the function execution and downstream HTTP call.

#### What to record

- Trace and span IDs on every span and on correlated log lines. W3C `traceparent` carries the trace ID and parent span ID between services and processes.
- A run or conversation ID using `gen_ai.conversation.id`, or your own attribute, so you can group all traces for one agent run or user session.
- The model's tool call ID where the provider sends one, using `gen_ai.tool.call.id` (Development status), to pair the model's request with your function's response.
- A business-domain ID as a custom span attribute, such as the GitHub issue number, so you can query traces by the thing the agent was working on rather than by internal IDs alone.

In iii, the AI Agent Harness adds `iii.session.id` and `iii.message.id` to W3C baggage at the start of each turn. In the Node SDK (`iii-sdk` 0.25.0), `registerFunction` wraps each handler to read that baggage and trace context from the incoming call.

The wrapper records the span and its invocation events, so you don't need to add that instrumentation to your worker. You can use `setCurrentSpanAttribute` to add your custom attributes:

```typescript
import { setCurrentSpanAttribute } from '@iii-dev/helpers/observability'

// Inside your triage::get_issue function:
setCurrentSpanAttribute('github.issue.number', issueNumber)
```

The console's Traces screen then lets you group by session or filter by that attribute without any backend configuration.

**Limitation:** Baggage travels with every outbound call from instrumented code, including a worker's `fetch()` calls to external services, even when trace export is not configured. The AI Agent Harness also adds session names and message previews to baggage by default, so audit what a call sends before it goes out, not just before you turn on export. The OTel Collector's redaction processor only handles exported span data. It cannot redact baggage headers already sent to external services.

### 2. Trace every hop your agent can reach

The model chooses which tools to call at runtime. Those tools may reach the GitHub API, an internal database, a state store, a message queue, or another system connected to the worker. Trace those downstream calls, not just the inbound trigger. Otherwise, calls outside the paths you tested may go unrecorded.

#### What to record

- The inbound HTTP or trigger context that starts the turn, as the root span.
- Outbound calls to every dependency a tool can call: HTTP APIs, databases, cache or state stores, queues, and any other integration point the system exposes.
- W3C `traceparent` on each hop that crosses a process or language boundary, so the trace doesn't break when a call reaches an uninstrumented service.
- Baggage entries only for IDs you want propagated to every downstream span.

In iii, calls between workers carry `traceparent` and baggage automatically. A Python worker called by a TypeScript agent shows up in the same trace without manual header injection. The HTTP worker accepts an incoming `traceparent` on inbound requests, the queue worker preserves it on each message, and outbound `fetch` calls in Node.js workers are traced automatically.

**Limitation:** Any process that drops `traceparent` starts a new trace.

### 3. Nest each tool call under its agent turn

A flat list of spans with similar timestamps can't answer the question "which model response led to which tool call?" Nesting tool spans under the agent turn makes the sequence easier to read in a waterfall view. You can also query the spans by parent span ID. Without that hierarchy, you have to reconstruct the sequence from timestamps, which is unreliable when calls overlap.

#### What to record

- One span per operation, named for what it does: a turn span for the overall agent step, a model call span for each call to the LLM, and an execute span for each tool invocation.
- `gen_ai.operation.name` on each span, so a backend can group spans by operation type across traces.
- Tool spans as children of the agent turn span, positioned after the model call span that generated the request, so the causal order is visible in the trace tree.

In iii, the `router::chat` span and each execute span are nested under the AI Agent Harness turn span automatically. The console waterfall shows the turn at the top, with model and tool calls indented beneath it. Each tool call appears after the model call that requested it.

<figure>
  <img src="../../assets/blog/ai-agent-observability-best-practices/console-waterfall.png" alt="The iii console waterfall for one agent turn, with router::chat and triage::apply_label nested under harness::turn step and the outbound GitHub POST nested under the tool span." />
  <figcaption>Console waterfall with <code>router::chat</code> and <code>execute triage::apply_label</code> as siblings under the turn span, and the outbound GitHub fetch span under the execute span.</figcaption>
</figure>

**Limitation:** The GenAI semantic convention span names are Development status, and iii uses its own naming scheme (`execute`, `router::chat`). Map iii span names to your backend's grouping rules rather than expecting them to match OTel conventions exactly.

### 4. Record the outcome of every tool call

The first question after any failure is "did the call succeed?" A span without a status or error type can't answer it.

Record outcomes beyond success or failure. A rate limit, a 403, an empty result, and a held approval each call for a different next step. If you record only whether an exception was thrown, you lose those distinctions.

#### What to record

- Span status left Unset on success, or Error with `error.type` on failure, so backends can filter and alert on error spans without parsing message text.
- A summary of the result or its size, rather than the full response body on every span. Full bodies belong in span events, and only when you've decided you need them.
- The attempt number when a call is retried, so a slow-but-successful retry doesn't look like a single fast call.
- Approval holds and refusals as their own outcome category, because they require human action rather than a code fix. See the [OpenCode vs Claude Code vs iii comparison](https://iii.dev/blog/opencode-vs-claude-code-vs-iii/) for how approvals appear in the traces pane.

In iii, the SDK records the function's return value as an `iii.invocation.output` span event with an `iii.payload.ok` field indicating success or failure. A turn held for approval shows `iii.turn.status` as `awaiting_functions`.

<figure>
  <img src="../../assets/blog/ai-agent-observability-best-practices/apply-label-error.png" alt="The iii console detail panel for execute triage::apply_label, showing an error badge, the invocation input with issue_number 9999, and the invocation output with error GitHub 404 and payload ok false." />
  <figcaption>Error details for <code>execute triage::apply_label</code>, showing span status Error and <code>iii.payload.ok</code> set to false in the expanded <code>iii.invocation.output</code> event.</figcaption>
</figure>

**Limitation:** Result bodies can be large and may contain sensitive data. The OTel Collector's [redaction processor](https://github.com/open-telemetry/opentelemetry-collector-contrib/tree/main/processor/redactionprocessor) and [transform processor](https://github.com/open-telemetry/opentelemetry-collector-contrib/tree/main/processor/transformprocessor) can strip or mask span event content before it reaches a hosted backend.

### 5. Measure every step's duration

Model streaming, queue handoffs, and approval holds can all add to agent latency. A 12-second turn doesn't tell you which step caused the delay. For example, span timings might show 8 seconds waiting for the model, 3 seconds in a queue, and 1 second executing the tool.

#### What to record

- Start and end time on every span. OTel SDKs record these automatically for spans they create.
- Time to first token on model calls, where the provider reports it, as a separate attribute or event.
- Queue waits and approval holds as their own spans so they're not counted against tool execution time in aggregated latency metrics.

In iii, every span has timing. Queue handoffs appear as 2 linked spans: a short "enqueue" span when the message is published, and a separate span when the worker picks it up and starts processing.

The console waterfall (shown under practice 3) draws each step's duration as a proportional bar, so you can see which step took the longest.

**Limitation:** iii's automatic spans end when the response object is returned, before the response body is consumed. For streaming responses, the span end time reflects the start of streaming, so account for that boundary when measuring end-to-end tool latency.

### 6. Put provider-reported token usage on the model span

Token counts recorded on the model span let you calculate cost per agent turn, identify which tools drive the most re-prompting, and catch prompt bloat.

Providers report token counts differently, and frameworks may normalize them in ways the attribute names don't make clear.

#### What to record

- Input and output totals using `gen_ai.usage.input_tokens` and `gen_ai.usage.output_tokens`. The OTel input total includes cached tokens.
- Cache reads and writes using `gen_ai.usage.cache_read.input_tokens` and `gen_ai.usage.cache_write.input_tokens`, where the provider reports them.
- Reasoning tokens as a separate attribute where the provider reports them (OpenAI o-series models, for example).
- Any cost estimate labeled explicitly as an estimate, not a billed amount. Use the provider's dashboard for actual charges.

In iii, the llm-router records available `gen_ai.usage.*` fields on the `execute router::chat` span. For the pinned OpenAI and Anthropic adapters, the input count is normalized to uncached tokens. Cache read and write counts are recorded as `gen_ai.usage.cache_read_tokens` and `gen_ai.usage.cache_write_tokens`.

These fields use the `gen_ai.*` prefix but are iii extensions, not names defined by the OTel GenAI conventions. For a worked example of per-task cost estimation, see [Running a coding agent for a dollar](https://iii.dev/blog/coding-agent-cost/).

**Limitation:** Raw Anthropic input counts exclude cached tokens while raw OpenAI input counts include them. iii normalizes the pinned adapters to uncached input. When mapping iii's input count to the OTel total (which includes cached tokens), add the cache slice without double-counting. Cross-reference against provider billing for any figure that affects a budget decision.

### 7. Decide what content to capture and redact it in 2 places

Tool arguments and prompts can help explain a failure, but they can also contain sensitive data.

Defaults vary by framework. Claude Code redacts prompts and tool content unless you opt in, while the OpenAI Agents SDK captures sensitive data unless you turn it off with `trace_include_sensitive_data=false`.

Whichever default your framework uses, choose what to capture based on your debugging needs and data-handling requirements.

#### What to record

- Always: trace and span IDs, tool names, span status, and payload sizes.
- Only where needed for debugging: function arguments, results, and prompts. The OTel GenAI conventions treat `gen_ai.tool.call.arguments` and `gen_ai.tool.call.result` as opt-in for exactly this reason.
- Never: credentials, tokens, or secrets in span attributes, span events, or baggage.

In iii, worker handler inputs and outputs are recorded as span events by default. Keys whose names contain patterns like `api_key`, `apikey`, `api-key`, `password`, `secret`, `credential`, `authorization`, `auth_token`, `access_token`, `refresh_token`, `bearer`, `private_key`, or `client_secret` are masked automatically before the event is written. A key that is exactly `token`, or ends in `_token` or `-token`, is also masked. Values under other keys, including prompt content, are kept as written.

To demonstrate masking behavior during development, use a dummy field like `test_api_key` with a placeholder value. Never log a real credential to verify the mask is working.

**Limitation:** Key-name matching won't catch a credential embedded inside a freeform string value. Redact sensitive data before it reaches the span event, and apply the OTel Collector's redaction processor as a second layer before sending spans to any external backend. The Collector only redacts exported data. It does not change the local copy, which retains anything the SDK's key-name masking did not remove. The local iii SQLite archive keeps that copy for up to 30 days or until the 1 GiB archive cap is reached, whichever comes first.

### 8. Keep traces as long as you need them and export the rest

A trace retained for 30 days tells you what happened in last week's incident. A trace retained for 2 hours tells you what happened this morning and nothing else.

Set retention explicitly. Choose sampling rules just as carefully. Head sampling can discard a trace before the request finishes, including your only record of a failure.

#### What to record

- A retention period for each signal (traces, logs, and metrics) chosen based on your actual debugging and compliance needs.
- An error-aware tail-sampling rule if you use sampling at all. Head sampling must not discard error spans before you know they're errors.
- An export target for long-term storage in an OTel-compatible backend, for any traces you need beyond local retention.

In iii engine 0.25.0, completed traces are stored in SQLite with a 30-day age limit and a 1 GiB cap. You can change both defaults through the `iii-observability` configuration entry. Logs and metrics are held in memory for up to 1 hour, subject to count limits (1,000 log records by default). Default trace sampling is 100%. For storage beyond 30 days, export to an OTel-compatible service.

To change retention or enable export after the first boot, edit the persisted configuration entry rather than `config.yaml`. `config.yaml` supplies the entry's initial values on first boot; later starts use the persisted entry.

`configuration::set` replaces the stored value; it does not merge changes. If you submit only the fields you want to change, every omitted field is dropped. Omitting `enabled` turns tracing off rather than adding an export target. Read the current value, apply your changes, and write back the full object:

```bash
# Read the current entry:
iii call configuration::get id=iii-observability

# Take the returned "value" object, change exporter/logs_exporter/endpoint in it,
# and set the complete object back:
iii call configuration::set --json '{"id": "iii-observability", "value": { /* full object, with your changes */ }}'
```

On engine 0.25.x the command is `iii trigger`, with the same arguments. Then restart the engine. Changes to `exporter` and `endpoint` take effect at the next engine start.

**Limitation:** Logs and metrics have their own exporter settings (`logs_exporter`, `metrics_exporter`) separate from the trace exporter. If you need all 3 signals in an external backend, configure each one explicitly.

## Tracing one tool call end to end in iii

The walkthrough uses a TypeScript worker named `triage` with 2 functions. `triage::get_issue` fetches issue metadata from the GitHub REST API, and `triage::apply_label` applies a label to an issue. The AI Agent Harness agent runs against a test repository with a fine-grained token scoped to that repository.

### The setup

After installing iii and adding the AI Agent Harness stack, the worker registers both functions and makes direct HTTPS calls to `api.github.com`:

```typescript
import { registerWorker } from 'iii-sdk'
import { setCurrentSpanAttribute } from '@iii-dev/helpers/observability'

const worker = registerWorker(undefined, { workerName: 'triage' })

worker.registerFunction('triage::get_issue', async ({ owner, repo, issue_number }) => {
  // Attach the business ID to the active span
  setCurrentSpanAttribute('github.issue.number', issue_number)
  const response = await fetch(
    `https://api.github.com/repos/${owner}/${repo}/issues/${issue_number}`,
    { headers: { Authorization: `Bearer ${process.env.GITHUB_TOKEN}` } },
  )
  return response.json()
})

worker.registerFunction('triage::apply_label', async ({ owner, repo, issue_number, label }) => {
  setCurrentSpanAttribute('github.issue.number', issue_number)
  const response = await fetch(
    `https://api.github.com/repos/${owner}/${repo}/issues/${issue_number}/labels`,
    {
      method: 'POST',
      headers: {
        Authorization: `Bearer ${process.env.GITHUB_TOKEN}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ labels: [label] }),
    },
  )
  if (!response.ok) throw new Error(`GitHub ${response.status}`)
  return response.json()
})
```

**Note:** `GITHUB_TOKEN` is read from the environment and placed directly in the outbound request's `Authorization` header. 2 separate mechanisms keep it out of this trace. Practice 7's payload masking only inspects the recorded inputs and outputs of `get_issue` and `apply_label`, and neither contains `GITHUB_TOKEN`. The automatic HTTP client span records only the `content-type` and `accept` request headers.

This fixed allowlist excludes `authorization`, so the span does not record the token. Neither mechanism would catch a credential placed somewhere else, such as a header added to that allowlist or a token echoed back into a function's own return value.

### The default trace

After installation, run 1 agent turn and open Traces in the console to see the full turn. No configuration changes are needed. The `execute triage::get_issue` span appears as a child of the AI Agent Harness turn, with the `router::chat` span alongside it showing `gen_ai.usage.input_tokens` and `gen_ai.usage.output_tokens`.

Below `execute triage::get_issue`, the outbound fetch to `api.github.com` appears as an automatic HTTP child span. The `github.issue.number` attribute set by `setCurrentSpanAttribute` appears in the `execute triage::get_issue` span's Attributes tab.

<figure>
  <img src="../../assets/blog/ai-agent-observability-best-practices/get-issue-attributes.png" alt="The iii console with the chat on the left and Traces on the right, the triage::get_issue span selected and its Attributes tab listing github.issue.number, iii.message.id, iii.session.id, and iii.function.id." />
  <figcaption>Attributes for <code>execute triage::get_issue</code>, including <code>iii.session.id</code>, <code>iii.message.id</code>, <code>github.issue.number</code>, and span status.</figcaption>
</figure>

### Adding a log line

A single `Logger` call from `@iii-dev/helpers/observability` writes a structured log that correlates with the span:

```typescript
import { Logger } from '@iii-dev/helpers/observability'

const logger = new Logger()

// Inside get_issue, after the fetch:
const issue = await response.json()
logger.info('Fetched issue', { issue_number, title: issue.title })
return issue
```

The log record carries the same `trace_id` and `span_id` as the `execute triage::get_issue` span, alongside the `issue_number` and `title` fields. Confirm this by calling `engine::logs::list`.

### The failing case

Running a second turn that calls `triage::apply_label` with an issue number that doesn't exist in the test repository returns a GitHub 404. The `execute triage::apply_label` span shows span status Error. The `iii.payload.ok` field in the `iii.invocation.output` span event is false, and the HTTP child span shows status 404, as in the error screenshot under practice 4.

The trace gives you the record for diagnosing the root cause. For controlling what an agent can do before it acts, see [What stops your coding agent from doing something it can't undo?](https://iii.dev/blog/what-stops-your-coding-agent-doing-something-it-cant-undo/)

### Export

To send traces to Jaeger (or any OTel-compatible backend) while keeping them in the console, update the persisted configuration entry to set `exporter: both` and add your Collector endpoint. Then restart the engine.

`configuration::set` replaces the whole stored value rather than merging, so read the current value first and send back the full object with your changes:

```bash
iii call configuration::get id=iii-observability
# edit exporter/logs_exporter/endpoint in the returned "value" object, then:
iii call configuration::set --json '{"id": "iii-observability", "value": { /* full object, with your changes */ }}'
```

With the [OTel Collector (contrib)](https://github.com/open-telemetry/opentelemetry-collector-contrib) and [Jaeger](https://www.jaegertracing.io/docs/latest/getting-started/) running locally, a fresh agent turn produces a trace visible in both the iii console and Jaeger under the same `trace_id`. The Collector configuration is also where you add the redaction processor for any span event content you want stripped before it reaches a hosted backend.

<figure>
  <img src="../../assets/blog/ai-agent-observability-best-practices/console-timeline.png" alt="The iii console timeline for trace 5378e09: harness::turn step, router::chat, provider::anthropic::stream, and triage::get_issue laid out over 6.03 seconds." />
  <figcaption>The exported turn in the iii console, trace <code>5378e09</code>.</figcaption>
</figure>

<figure>
  <img src="../../assets/blog/ai-agent-observability-best-practices/jaeger-trace.png" alt="Jaeger showing harness: execute harness::send, trace 5378e09, 6.0 seconds across 12 services, with session-manager, state, queue, and llm-router spans nested beneath it." />
  <figcaption>The same trace ID in Jaeger after export.</figcaption>
</figure>

## What traces still won't tell you

**Why the model chose a tool.** A trace records what ran and what came back. It does not record the model's reasoning. Text in a reasoning field is model output, not a reliable explanation of why the model chose a tool.

**What happened outside the trace.** iii's automatic spans capture request metadata and response status codes. They do not capture response bodies or what the downstream service did with the request. An outbound request alone doesn't confirm that the intended change happened. Tracing an action and recovering from it are separate problems.

**Gaps in coverage.** A missing trace doesn't prove that nothing ran. Sampling, retention limits, missing instrumentation, or a hop that drops `traceparent` can leave gaps.

**Changing attribute names.** The OpenTelemetry GenAI conventions carry Development status as of October 2026. Attribute names may change before they reach Stable. iii's spans use its own field names alongside OTel names for cache tokens, reasoning, and cost estimates. Recheck dashboards and alerts that query these attributes after a breaking update to iii or the OTel conventions.

Traces give you the record. Deciding what goes into that record, how long it stays, and where it lives is still your call.

## What else to know about tracing agent tool calls

The questions below come up frequently when developers are setting up agent tracing for the first time.

### What are traces in AI agents?

A trace is a tree of timed spans that records the instrumented steps in an agent execution, such as model calls, tool calls, and handoffs. The spans share a `trace_id`. The waterfall screenshot under practice 3 shows what that looks like, with the turn at the root, model and tool calls nested below it, and outbound service calls nested below those. Each span has a start time, an end time, attributes, and optionally span events for richer data like function inputs and outputs.

### How does agent observability differ from LLM monitoring?

LLM monitoring watches model calls: latency, token counts, and errors at the inference boundary. Agent observability follows the whole run, including every tool the agent called, the downstream services those tools reached, and the handoffs between workers. OpenTelemetry's GenAI agent-span conventions use a similar hierarchy: an `invoke_agent` span is intended to be the parent of the agent's tool and task spans.

### Can an agent read its own traces?

In iii, yes. The engine exposes `engine::traces::list` and `engine::traces::tree`, which a worker can call to retrieve trace data for the current or past sessions. The AI Agent Harness only allows calls to functions it has been explicitly authorized to reach, so you can deliberately grant an agent access to trace functions. Grant read-only access to trace functions only when the agent needs it, with the same scrutiny you'd apply to other operational data.
