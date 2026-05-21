---
type: index
title: iii-observability
---

# iii-observability

OpenTelemetry-backed observability for the iii engine: distributed tracing, structured logs, metrics with rollups, alert rules, sampling configuration, and baggage propagation. Every surface is a callable engine function under `engine::*`, plus a single `log` reactive trigger that fires on every ingested log entry. The worker exposes 19 functions across 9 sub-namespaces — emit, listen, query stored telemetry, manage configuration, propagate context.

The worker is on by default (`config.enabled: true`). When disabled, the emit and read functions still register but become no-ops; the trigger never fires. Core config keys: `service_name` / `service_version` / `service_namespace` (OTel resource attributes), `exporter` (`memory` | `otlp` | `both`), `endpoint` (OTLP collector URL, default `http://localhost:4317`), `sampling_ratio` (`0.0`–`1.0`, default `1.0`), `memory_max_spans` (default `1000`), and per-pillar toggles (`metrics_enabled`, `logs_enabled`) plus retention/limit knobs (`metrics_retention_seconds`, `metrics_max_count`, `logs_max_count`, `logs_retention_seconds`, `logs_console_output`, `logs_sampling_ratio`). Most fields can be overridden via env vars (`OTEL_*`). Alert rules and the advanced sampling block are documented inline alongside the relevant skills.

- **Emit and listen** (`engine::log::*`, `log` trigger) — five severity-level emit functions and a reactive trigger that fires on every ingested log.
- **Query stored telemetry** (`engine::logs::*`, `engine::traces::*`, `engine::metrics::*`, `engine::rollups::*`) — list/tree/clear for the in-memory log + span store, list metrics with optional time bucketing, list metric rollup aggregations.
- **Inspect configuration** (`engine::sampling::*`, `engine::health::*`, `engine::alerts::*`) — view the active sampling rules, check engine health, list and re-evaluate alert rules.
- **Propagate context** (`engine::baggage::*`) — read and write OpenTelemetry baggage keys for cross-call context (note: writes don't propagate back to the caller — see the file).

## How-tos

### `engine::log::*` — emit log entries

- [`engine::log::info`, `warn`, `error`, `debug`, `trace`](iii://iii-observability/engine/log/log) — five emit functions sharing the same input shape (`message`, optional `data`/`trace_id`/`span_id`/`service_name`); only the severity differs.

### `log` triggers

- [React to ingested log entries](iii://iii-observability/engine/log/reactive-triggers) — register a handler bound to the `log` trigger type to fire on every (or filtered-by-level) log emitted via `engine::log::*` or stored via the OTel log pipeline.

### `engine::logs::*` and `engine::traces::*` — query stored telemetry

- [`engine::logs::list`, `engine::logs::clear`](iii://iii-observability/engine/logs/logs) — read or wipe the in-memory log store; filter by time range, trace correlation, or severity.
- [`engine::traces::list`, `engine::traces::tree`, `engine::traces::clear`](iii://iii-observability/engine/traces/traces) — list spans, walk a single trace as a parent/child tree, or wipe stored spans.

### `engine::metrics::*` and `engine::rollups::*` — metrics inspection

- [`engine::metrics::list`](iii://iii-observability/engine/metrics/list) — list metrics with aggregated stats; surfaces engine counters, SDK metrics, and optional time-bucketed values.
- [`engine::rollups::list`](iii://iii-observability/engine/rollups/list) — list metric rollup aggregations across 1-minute, 5-minute, and 1-hour windows.

### `engine::baggage::*` — context propagation

- [`engine::baggage::get`, `engine::baggage::set`, `engine::baggage::get_all`](iii://iii-observability/engine/baggage/baggage) — read and (locally) set OpenTelemetry baggage keys. **Note**: `baggage::set` does not propagate back to the caller — propagation must be done at the SDK/invocation level via baggage headers.

### `engine::sampling::*`, `engine::health::*`, `engine::alerts::*` — operational inspection

- [`engine::sampling::rules`](iii://iii-observability/engine/sampling/rules) — list the active sampling rules (default rate, parent-based flag, per-operation rules, rate limits).
- [`engine::health::check`](iii://iii-observability/engine/health/check) — return engine health (`status`, per-component breakdown, `timestamp`, `version`).
- [`engine::alerts::list`, `engine::alerts::evaluate`](iii://iii-observability/engine/alerts/alerts) — inspect configured alert rules and their current state, or manually trigger an evaluation pass.
