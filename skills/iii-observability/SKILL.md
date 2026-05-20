---
name: iii-observability
description: >-
  Integrates OpenTelemetry tracing, metrics, and logging into iii workers. Use when
  setting up distributed tracing, Prometheus metrics, custom spans, or connecting
  to observability backends.
---

# Observability

Comparable to: Datadog, Grafana, Honeycomb, Jaeger

## Key Concepts

Use the concepts below when they fit the task. Not every worker needs custom spans or metrics.

- Built-in **OpenTelemetry** support across all SDKs — every function invocation is automatically traced
- Install or enable engine observability with `iii worker add iii-observability`
- The engine exports traces, metrics, and logs via **OTLP** to any compatible collector
- Workers propagate **W3C trace context** automatically across function invocations
- `traceparent` and `baggage` can be extracted/injected for outbound HTTP and external systems
- **Prometheus** metrics are exposed on port 9464
- `registerWorker()` with `otel` config enables telemetry per worker
- **Custom spans** via `withSpan(name, opts, fn)` wrap async work with trace context
- **Custom metrics** via the OpenTelemetry API (`@opentelemetry/api` in Node, `opentelemetry` in Python/Rust) create counters and histograms
- **Logger** APIs produce structured logs that correlate with traces when OpenTelemetry is enabled
- Engine metrics and the observability worker are related but different: engine metrics describe runtime health; observability worker/export config controls traces, logs, and metrics export

## Architecture

The worker SDK generates spans, metrics, and logs during function execution. These flow to the engine, which exports them via OTLP to a collector (Jaeger, Grafana, Datadog). The engine also exposes a Prometheus endpoint on port 9464 for scraping.

## iii Primitives Used

| Primitive                    | Purpose                                       |
| ---------------------------- | --------------------------------------------- |
| `registerWorker(url, { otel })`        | Connect worker with telemetry config          |
| `Logger.info/warn/error/debug(message, data?)` | Emit structured SDK logs              |
| `withSpan(name, opts, fn)`   | Create a custom trace span                    |
| `currentTraceId()`           | Get active trace ID for correlation           |
| `injectTraceparent()`        | Inject W3C trace context into outbound calls  |
| `injectBaggage()` / `extractBaggage()` | Propagate W3C baggage where supported |
| `onLog(callback, { level })` | Subscribe to log events                       |
| `shutdown_otel()`            | Graceful shutdown of telemetry pipeline       |

## Reference Implementation

See [../references/observability.js](../references/observability.js) for the full working example — a worker with custom spans,

Also available in **Python**: [../references/observability.py](../references/observability.py)

Also available in **Rust**: [../references/observability.rs](../references/observability.rs)
metrics counters, trace propagation, and log subscriptions connected to an OTel collector.

## Common Patterns

Code using this pattern commonly includes, when relevant:

- Node: `registerWorker('ws://localhost:49134', { otel: { enabled: true, serviceName: 'my-svc' } })` — enable telemetry
- Python: `register_worker(address, InitOptions(otel=OtelConfig(enabled=True, service_name='my-svc')))` — enable telemetry
- Rust: `register_worker(address, InitOptions { otel: Some(OtelConfig { enabled: Some(true), service_name: Some("my-svc".into()), ..Default::default() }), ..Default::default() })`
- `new Logger().info('processing', { requestId })` / `Logger().info(...)` / `Logger::new().info("processing", Some(json!({...})))` — structured logs
- `withSpan('validate-order', {}, async (span) => { span.setAttribute('order.id', id); ... })` — custom span
- `metrics.getMeter('my-svc').createCounter('orders.processed')` — custom counter via `@opentelemetry/api`
- `metrics.getMeter('my-svc').createHistogram('request.duration')` — custom histogram via `@opentelemetry/api`
- `onLog((log) => { ... }, { level: 'warn' })` — subscribe to warnings and above
- `currentTraceId()` — get active trace ID for correlation with external systems
- `injectTraceparent()` — propagate trace context to outbound HTTP calls
- Disable OpenTelemetry: `registerWorker(url, { otel: { enabled: false } })` or `OTEL_ENABLED=false`
- Disable product telemetry separately with the current Disable Telemetry docs when the task is about anonymous product analytics rather than OpenTelemetry

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Enable `otel` in `registerWorker()` config to start collecting traces automatically
- Use `Logger` for structured logs instead of `console.log` / `print` when logs should appear in iii observability
- Add custom spans around expensive operations (DB queries, LLM calls, external APIs)
- Create domain-specific metrics (orders processed, payment failures, queue depth)
- Use `currentTraceId()` to correlate iii traces with external system logs
- Configure `iii-observability` in iii-config.yaml for engine-side exporter, sampling ratio, and alerts
- Point the OTLP endpoint at your collector (Jaeger, Grafana Tempo, Datadog Agent)
- Browser SDK has no OpenTelemetry export; emit browser telemetry through your frontend stack or call server-side iii functions that log/trace the event

## Engine Configuration

Install/enable iii-observability with `iii worker add iii-observability`; it must be configured in iii-config.yaml for engine-side traces, metrics, and logs. See [../references/iii-config.yaml](../references/iii-config.yaml) for the full annotated config reference.

## Pattern Boundaries

- For engine-side iii-observability YAML configuration, prefer `iii-engine-config`.
- For SDK init options and function registration, prefer `iii-functions-and-triggers`.
- For errors and error codes, prefer `iii-error-handling`.
- Stay with `iii-observability` when the primary problem is SDK-level telemetry: spans, metrics, logs, and trace propagation.

## When to Use

- Use this skill when the task is primarily about `iii-observability` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
