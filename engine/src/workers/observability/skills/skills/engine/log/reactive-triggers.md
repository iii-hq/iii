---
type: how-to
trigger_type: log
title: React to ingested log entries
---

# When to use

Register a `log` trigger when a function should run automatically every time a log entry lands in the engine's OTel log pipeline — emitted via `engine::log::*`, ingested via the OTLP collector, or recorded by any worker that uses the engine's structured logging. Each subscription receives the same OTel-shaped log record, so consumers can route by severity, attribute, or trace correlation.

Reach for it when:

- A specific severity (typically `error`) should page a human, send a Slack message, or open a ticket — register the handler with `level: "error"` and the trigger only fires on matching entries.
- You want a real-time fan-out of all log entries to a downstream sink (S3 archive, log analytics, custom transformer) without polling `engine::logs::list`.
- You want auditing: any log emitted anywhere in the engine should be recorded somewhere durable.

Use [`engine::logs::list`](iii://iii-observability/engine/logs/logs) instead when you need to **query** stored entries on demand rather than react to each as it arrives.

# Inputs

Registration is the standard two-step pattern: define the handler, then bind it to the `log` trigger.

## Handler function

Register any function id. The handler receives the OTel log record documented in **Outputs**.

```json
// iii.registerFunction — handler id only; no engine payload.
{ "id": "monitoring::on-error" }
```

## Trigger registration

```json
{
  "type":        "log",                        // required. Must be exactly "log".
  "function_id": "monitoring::on-error",       // required. Handler invoked when a log entry matches.
  "config": {
    "level": "error"                           // optional. One of "trace" | "debug" | "info" | "warn" | "error". Omit to fire on every level.
  }
}
```

`type` and `function_id` are required. `config.level`, when set, restricts firing to entries at that severity; when omitted, the trigger fires on every entry.

# Outputs

The handler receives the OTel-shaped log record:

```json
{
  "timestamp_unix_nano":          1716220800000000000,         // when the log was emitted (nanos since epoch)
  "observed_timestamp_unix_nano": 1716220800001000000,         // when the engine ingested it (nanos since epoch)
  "severity_number":              17,                          // OTel severity number (TRACE=1, DEBUG=5, INFO=9, WARN=13, ERROR=17)
  "severity_text":                "ERROR",                     // "TRACE" | "DEBUG" | "INFO" | "WARN" | "ERROR"
  "body":                         "request failed",            // The log message (whatever was passed as `message` to engine::log::*).
  "attributes":                   { "user_id": "u_1", "code": 500 },  // The structured `data` from the emit call, plus any pipeline-added attributes.
  "trace_id":                     "0123456789abcdef0123456789abcdef",  // null when no trace context was present.
  "span_id":                      "0123456789abcdef",          // null when no span context was present.
  "resource":                     { "service.name": "billing" }, // OTel resource attributes from the emitting service.
  "service_name":                 "billing",                   // Convenience field; same as resource["service.name"].
  "instrumentation_scope_name":   "iii-engine",
  "instrumentation_scope_version": "0.12.0"
}
```

- `timestamp_unix_nano` is in **nanoseconds** since the Unix epoch — divide by `1_000_000` for milliseconds.
- `severity_text` and `severity_number` always agree; use whichever is convenient.
- `trace_id`/`span_id` are `null` when the emit happened outside any active OTel context.

The handler's return value is **ignored**. Trigger invocations are spawned asynchronously after each log entry is stored — handler errors do not roll back the log entry or block the next firing.

# Worked example

Subscribe a handler that fires only on `error`-level entries:

```json
{
  "type":        "log",
  "function_id": "monitoring::on-error",
  "config":      { "level": "error" }
}
```

The typical pattern is one trigger per concern: a `level: "error"` trigger that pages on failures, a `level: "warn"` trigger that records to a metrics store, an unfiltered trigger (omit `level`) that archives every log to long-term storage. Branch inside the handler on `severity_text`, `service_name`, or specific keys in `attributes` to dispatch.

For runnable scaffolds covering these patterns, see the observability worker source and SDK examples in [the iii main repo](https://github.com/iii-hq/iii).

# Related

- [`engine::log::*`](iii://iii-observability/engine/log/log) — the emit functions whose calls fire this trigger.
- [`engine::logs::list`](iii://iii-observability/engine/logs/logs) — query stored entries on demand, complementing the reactive path.
- `iii-observability` worker config — when `logs_enabled: false` (default `false`; must be turned on in `iii-config.yaml`), the log pipeline is dormant and no trigger fires. The `level` config key (`trace` | `debug` | `info` | `warn` | `error`; default `info`) sets the **minimum** severity ingested; entries below it are dropped before reaching this trigger.
