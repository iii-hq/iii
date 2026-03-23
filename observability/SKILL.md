---
name: observability
description: >-
  Integrates OpenTelemetry tracing, metrics, and logging into iii workers. Use
  when setting up distributed tracing, Prometheus metrics, custom spans, or
  connecting to observability backends.
---

# Observability

Comparable to: Datadog, Grafana, Honeycomb, Jaeger

## Key Concepts

Use the concepts below when they fit the task. Not every worker needs custom spans or metrics.

- Built-in **OpenTelemetry** support across all SDKs — every function invocation is automatically traced
- The engine exports traces, metrics, and logs via **OTLP** to any compatible collector
- Workers propagate **W3C trace context** automatically across function invocations
- **Prometheus** metrics are exposed on port 9464
- SDK init with `otel` config enables telemetry per worker
- **Custom spans** via `withSpan(name, opts, fn)` wrap async work with trace context
- **Custom metrics** via `getMeter()` create counters and histograms

## Architecture

The worker SDK generates spans, metrics, and logs during function execution. These flow to the engine, which exports them via OTLP to a collector (Jaeger, Grafana, Datadog). The engine also exposes a Prometheus endpoint on port 9464 for scraping.

## iii Primitives Used

| Primitive                            | Purpose                                      |
| ------------------------------------ | -------------------------------------------- |
| `init(url, { otel })` | Connect worker with telemetry config         |
| `withSpan(name, opts, fn)`           | Create a custom trace span                   |
| `getTracer()`                        | Access OpenTelemetry Tracer directly         |
| `getMeter()`                         | Access OpenTelemetry Meter for custom metrics|
| `currentTraceId()`                   | Get active trace ID for correlation          |
| `injectTraceparent()`                | Inject W3C trace context into outbound calls |
| `onLog(callback, { level })`         | Subscribe to log events                      |
| `shutdown_otel()`                    | Graceful shutdown of telemetry pipeline      |

## Reference Implementation

See [../references/observability.js](../references/observability.js) for the full working example — a worker with custom spans,
metrics counters, trace propagation, and log subscriptions connected to an OTel collector.

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `init('ws://localhost:49134', { otel: { enabled: true, serviceName: 'my-svc' } })` — enable telemetry
- `withSpan('validate-order', {}, async (span) => { span.setAttribute('order.id', id); ... })` — custom span
- `getMeter().createCounter('orders.processed')` — custom counter metric
- `getMeter().createHistogram('request.duration')` — custom histogram metric
- `onLog((log) => { ... }, { level: 'warn' })` — subscribe to warnings and above
- `currentTraceId()` — get active trace ID for correlation with external systems
- `injectTraceparent()` — propagate trace context to outbound HTTP calls
- Disable telemetry: `init(url, { otel: { enabled: false } })` or `OTEL_ENABLED=false`

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Enable `otel` in init config to start collecting traces automatically
- Add custom spans around expensive operations (DB queries, LLM calls, external APIs)
- Create domain-specific metrics (orders processed, payment failures, queue depth)
- Use `currentTraceId()` to correlate iii traces with external system logs
- Configure `OtelModule` in iii-config.yaml for engine-side exporter, sampling ratio, and alerts
- Point the OTLP endpoint at your collector (Jaeger, Grafana Tempo, Datadog Agent)

## Pattern Boundaries

- For engine-side OtelModule YAML configuration, prefer `engine-config`.
- For SDK init options and function registration, prefer `functions-and-triggers`.
- Stay with `observability` when the primary problem is SDK-level telemetry: spans, metrics, logs, and trace propagation.
