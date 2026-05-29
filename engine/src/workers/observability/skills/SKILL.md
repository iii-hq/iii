---
name: iii-observability
description: >-
  OpenTelemetry-backed tracing, structured logs, metrics with rollups, alerts,
  sampling, and baggage for the engine — emit and query telemetry through
  `engine::*` functions and react to logs with a `log` trigger.
---

# iii-observability

The `iii-observability` worker provides OpenTelemetry-backed observability for the iii engine: distributed tracing, structured logs, metrics with rollups, alert rules, sampling configuration, and baggage propagation. Every surface is a callable `engine::*` function, plus a single `log` reactive trigger that fires on every ingested log entry. Functions span nine sub-namespaces — emit, query stored telemetry, inspect configuration, and propagate context.

The worker is on by default (`enabled: true`). When disabled, the emit and read functions still register but become no-ops and the trigger never fires. Core config: `service_name`/`service_version`/`service_namespace` (OTel resource attributes), `exporter` (`memory` | `otlp` | `both`), `endpoint` (OTLP collector URL, default `http://localhost:4317`), `sampling_ratio` (`0.0`–`1.0`), `memory_max_spans`, and per-pillar toggles/limits (`metrics_enabled`, `logs_enabled`, retention and count caps). Most fields accept `OTEL_*` env overrides. The in-memory query functions (`logs`, `traces`) require the `memory` or `both` exporter.

## When to Use

- Emit structured logs or read back stored logs/spans/metrics from inside a function instead of shelling out to a collector.
- React in real time to log entries (page on errors, archive everything) without polling.
- Inspect engine health, active sampling rules, or alert state operationally.
- Propagate OpenTelemetry baggage across calls for cross-cutting context.

## Boundaries

- `engine::baggage::set` does not propagate back to the caller — baggage propagation happens at the SDK/invocation level via headers.
- The in-memory query functions return nothing useful unless the `memory` (or `both`) exporter is configured; with `otlp`-only the data lives in your collector.
- When `logs_enabled` is off the log pipeline is dormant and the `log` trigger never fires; the ingest `level` sets the minimum severity stored.
- This worker observes telemetry — it is not a general event bus (`iii-pubsub`) or durable queue (`iii-queue`).

## Functions

- `engine::log::info`, `engine::log::warn`, `engine::log::error`, `engine::log::debug`, `engine::log::trace` — emit a log entry at the named severity; same input shape, only the level differs.
- `engine::logs::list` — read stored OTel logs, filtered by time, trace correlation, or severity.
- `engine::logs::clear` — wipe the in-memory log store.
- `engine::traces::list` — list stored spans.
- `engine::traces::tree` — walk a single trace as a parent/child tree.
- `engine::traces::group_by` — aggregate stored spans by an attribute value (counts, duration, errors per group).
- `engine::traces::clear` — wipe stored spans.
- `engine::metrics::list` — list metrics with aggregated stats and optional time bucketing.
- `engine::rollups::list` — list metric rollup aggregations across 1-minute, 5-minute, and 1-hour windows.
- `engine::baggage::get`, `engine::baggage::get_all` — read one or all OpenTelemetry baggage keys.
- `engine::baggage::set` — set a baggage key locally (does not propagate to the caller).
- `engine::sampling::rules` — list the active sampling rules.
- `engine::health::check` — return engine health (status, per-component breakdown, version).
- `engine::alerts::list` — inspect configured alert rules and current state.
- `engine::alerts::evaluate` — manually run an alert evaluation pass.

## Reactive triggers

Bind a `log` trigger when a function should run every time a log entry lands in the engine's OTel log pipeline — emitted via `engine::log::*`, ingested via OTLP, or recorded by any worker using structured logging. Each subscription receives the same OTel-shaped record, so handlers can route by severity, attribute, or trace correlation.

Reach for it when:

- A specific severity (typically `error`) should page a human, post to Slack, or open a ticket.
- You want real-time fan-out of log entries to a downstream sink (archive, analytics, transformer) without polling `engine::logs::list`.

Use `engine::logs::list` instead when you need to query stored entries on demand rather than react to each as it arrives.

### How to bind

1. Register a handler: `iii.registerFunction('monitoring::on-error', handler)`.
2. Register the trigger:

```typescript
iii.registerTrigger({
  type: 'log',
  function_id: 'monitoring::on-error',
  config: {
    level: 'error',  // optional. trace|debug|info|warn|error. Omit to fire on every level.
  },
})
```

The `log` trigger only fires when the logs pipeline is enabled, and `level` filters to that minimum severity. The handler's return value is ignored; invocations are spawned asynchronously after each entry is stored.

For the OTel log record shape, call `iii get function info` on the trigger type or handler function id.
