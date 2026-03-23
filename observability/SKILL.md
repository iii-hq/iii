---
name: observability
description: >-
  Integrates OpenTelemetry traces, metrics, and logs into iii workers using
  the SDK. Use when adding custom spans, collecting metrics, propagating
  traces, subscribing to logs, or configuring the OtelModule in the engine.
---

# Observability

Comparable to: Datadog, Grafana, Jaeger

## Key Concepts

- Every function invocation is **automatically traced** by the SDK
- **Custom spans** wrap async work with `withSpan('name', opts, fn)`
- **Custom metrics** use `getMeter()` to create counters and histograms
- **Trace propagation** via `currentTraceId()`, `currentSpanId()`
- **Log subscriptions** via `iii.onLog(callback, { level })` in the SDK
- **OtelModule** in iii-config.yaml configures the engine-side exporter, sampling, and alerts
- **Worker metrics** (CPU, memory, uptime) are reported automatically when `enableMetricsReporting: true`

## iii Primitives Used

| Primitive | Purpose |
|-----------|---------|
| `registerWorker(url, { otel })` | Connect worker with telemetry config |
| `withSpan(name, opts, fn)` | Create a custom trace span |
| `getMeter()` | Access OpenTelemetry Meter for custom metrics |
| `getTracer()` | Access OpenTelemetry Tracer |
| `getLogger()` | Access OpenTelemetry Logger |
| `currentTraceId()` | Get active trace ID |
| `currentSpanId()` | Get active span ID |
| `iii.onLog(callback, { level })` | Subscribe to log events |
| `shutdownOtel()` | Graceful shutdown of telemetry pipeline |

## Common Patterns

- `registerWorker('ws://localhost:49134', { otel: { enabled: true, serviceName: 'my-svc' } })` -- enable telemetry
- `withSpan('validate-order', {}, async (span) => { span.setAttribute('order.id', id); ... })` -- custom span
- `getMeter().createCounter('orders.processed')` -- custom counter
- `iii.onLog((log) => { ... }, { level: 'warn' })` -- subscribe to warnings and above
- `currentTraceId()` -- get active trace ID for correlation
- Disable: `registerWorker(url, { otel: { enabled: false } })` or `OTEL_ENABLED=false`
- Engine config: `OtelModule` with `exporter: otlp`, `endpoint`, `sampling_ratio`
- Prometheus endpoint: port 9464

## Reference

See [references/REFERENCE.md](references/REFERENCE.md) for the full API reference with TypeScript, Python, and Rust examples.

## Pattern Boundaries

- For engine-side OtelModule YAML configuration, see `engine-config`.
- For function registration and triggers, see `functions-and-triggers`.
- Stay with `observability` when the goal is SDK-level telemetry: spans, metrics, logs, and trace propagation.
