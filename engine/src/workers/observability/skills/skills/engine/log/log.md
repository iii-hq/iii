---
type: how-to
functions: [engine::log::info, engine::log::warn, engine::log::error, engine::log::debug, engine::log::trace]
title: Emit a log entry through the engine's OpenTelemetry pipeline
---

# When to use

The five emit functions all do the same thing — record a structured log entry in the engine's OTel log pipeline — and differ only in **severity**. Pick by how a downstream consumer (alerting, log routing, the `log` reactive trigger) should treat the message.

| Function                | Severity | OTel severity_number | Typical use                                                |
|-------------------------|----------|----------------------|------------------------------------------------------------|
| `engine::log::trace`    | TRACE    | 1                    | Per-step diagnostic noise; almost always sampled out.      |
| `engine::log::debug`    | DEBUG    | 5                    | Developer-facing detail useful during incidents.            |
| `engine::log::info`     | INFO     | 9                    | Normal operational events; the default for most callers.    |
| `engine::log::warn`     | WARN     | 13                   | Recoverable problems worth surfacing in dashboards.         |
| `engine::log::error`    | ERROR    | 17                   | Failures that warrant alerting or operator attention.       |

Reach for these when:

- A handler should record progress, decisions, or failure detail in a way that survives in the engine's log store and propagates to OTLP exporters when configured.
- You want a `log` reactive trigger (see [React to ingested log entries](iii://iii-observability/engine/log/reactive-triggers)) to fire on the message — every emit goes through the same pipeline that feeds the trigger.
- You want trace correlation: pass `trace_id`/`span_id` so the entry shows up under the right span in `engine::traces::tree`.

Use `engine::traces::*` (`iii://iii-observability/engine/traces/traces`) instead when the goal is to record timing/structure of an operation — spans are richer than logs for that.

# Inputs

All five functions accept the same `OtelLogInput`:

```json
{
  "message":      "request handled",         // required. The log body.
  "data":         { "user_id": "u_1" },      // optional. Structured attributes attached to the log entry.
  "trace_id":     "0123456789abcdef0123456789abcdef",  // optional. Correlation to a specific trace; leave unset to inherit the current OTel context.
  "span_id":      "0123456789abcdef",        // optional. Correlation to a specific span within the trace.
  "service_name": "billing"                  // optional. Override the service name attribute on this entry; defaults to the worker's configured `service_name`.
}
```

`message` is required. All other fields are optional.

# Outputs

The functions return no value (`FunctionResult::NoResult`); on the wire the response is `null`. The observable effects happen on the side: the entry is stored in memory (subject to `logs_max_count` and `logs_retention_seconds`), the `log` reactive trigger fires, and — when `logs_console_output` is on — the entry is printed to stderr.

When the worker is disabled (`config.enabled: false`) or `logs_enabled: false`, the call is silently a no-op: no storage, no trigger fan-out, no console print, no export.

# Worked example

Emit an `error` entry with a structured payload and trace correlation:

```json
{
  "message":      "payment authorization failed",
  "data":         { "user_id": "u_1", "amount_cents": 4200, "code": "card_declined" },
  "trace_id":     "0123456789abcdef0123456789abcdef",
  "span_id":      "0123456789abcdef",
  "service_name": "billing"
}
```

The typical pattern is one call per significant operational event: an `info` on success, a `warn` on a recoverable problem, an `error` on a failure that should alert. Pass `data` for structured fields the log query and trigger handler can branch on (`level`, `user_id`, `request_id`, etc.) — those land in the entry's `attributes` map and are queryable via `engine::logs::list`.

For runnable scaffolds (TypeScript, Python, Rust) plus the standard `Logger` wrapper that batches these calls, see the observability worker source and SDK examples in [the iii main repo](https://github.com/iii-hq/iii).

# Related

- [React to ingested log entries](iii://iii-observability/engine/log/reactive-triggers) — register a `log` trigger to run a function every time `engine::log::*` is called.
- [`engine::logs::list`](iii://iii-observability/engine/logs/logs) — query the stored entries this function produces.
- [`engine::traces::*`](iii://iii-observability/engine/traces/traces) — for span-shaped telemetry; pair with logs by passing the same `trace_id`.
