# Observability -- API Reference

Source: https://iii.dev/docs/advanced/telemetry

## SDK Initialization with Telemetry

### TypeScript/Node.js

```typescript
import { registerWorker } from 'iii-sdk'
import { withSpan, getMeter, getTracer, getLogger, currentTraceId, currentSpanId, shutdownOtel } from 'iii-sdk/telemetry'

const iii = registerWorker('ws://localhost:49134', {
  otel: {
    enabled: true,
    serviceName: 'my-service',
  },
  enableMetricsReporting: true,
})
```

### Python

```python
from iii import InitOptions, register_worker
from iii.telemetry_types import OtelConfig
from iii.telemetry import (
    with_span, get_tracer, get_meter, get_logger,
    current_trace_id, shutdown_otel_async
)

iii = register_worker(
    address="ws://localhost:49134",
    options=InitOptions(
        otel=OtelConfig(
            enabled=True,
            service_name="my-service",
        ),
        enable_metrics_reporting=True,
    ),
)
```

### Rust

```rust
use iii_sdk::{InitOptions, OtelOptions, register_worker};
use iii_sdk::telemetry::{with_span, get_logger_provider};

let iii = register_worker(
    "ws://localhost:49134",
    InitOptions {
        otel: Some(OtelOptions {
            enabled: true,
            service_name: "my-service".into(),
            ..Default::default()
        }),
        ..Default::default()
    },
)?;
```

---

## Custom Spans

### TypeScript

```typescript
iii.registerFunction({ id: 'orders::process' }, async (data) => {
  return withSpan('validate-order', {}, async (span) => {
    span.setAttribute('order.id', data.orderId)
    const valid = await validateOrder(data)
    span.setAttribute('order.valid', valid)
    return { status_code: 200, body: { valid } }
  })
})
```

### Python

```python
async def process_order(data):
    async def validate(span):
        span.set_attribute("order.id", data["orderId"])
        valid = await validate_order(data)
        span.set_attribute("order.valid", valid)
        return {"status_code": 200, "body": {"valid": valid}}

    return await with_span("validate-order", validate)
```

### Rust

```rust
with_span("validate-order", |span| async move {
    span.set_attribute("order.id", input["orderId"].as_str().unwrap_or_default());
    let valid = validate_order(&input).await?;
    span.set_attribute("order.valid", valid);
    Ok(json!({ "status_code": 200, "body": { "valid": valid } }))
}).await
```

---

## Custom Metrics

### TypeScript

```typescript
const meter = getMeter()
const counter = meter.createCounter('orders.processed')
const histogram = meter.createHistogram('orders.duration_ms')

counter.add(1, { status: 'success' })
histogram.record(elapsed, { endpoint: '/orders' })
```

### Python

```python
meter = get_meter()
if meter:
    counter = meter.create_counter("orders.processed")
    histogram = meter.create_histogram("orders.duration_ms")
    counter.add(1, {"status": "success"})
    histogram.record(elapsed, {"endpoint": "/orders"})
```

---

## Trace Propagation

### TypeScript

```typescript
const traceId = currentTraceId()
const spanId = currentSpanId()
```

### Python

```python
trace_id = current_trace_id()
```

---

## Log Subscriptions

### TypeScript

```typescript
iii.onLog((log) => {
  console.log(`[${log.severity_text}] ${log.body}`)
}, { level: 'warn' })
```

### Rust -- Logger Access

```rust
use opentelemetry::logs::{Logger, LogRecord, Severity};

if let Some(provider) = get_logger_provider() {
    let logger = provider.logger("my-service");
    let mut record = LogRecord::default();
    record.set_severity_number(Severity::Info);
    record.set_body("Order processed successfully".into());
    logger.emit(record);
}
```

---

## Auto-Collected Worker Metrics

| Metric | SDKs |
|--------|------|
| `iii.worker.cpu.percent` | Node.js, Python |
| `iii.worker.memory.rss` | Node.js, Python |
| `iii.worker.memory.heap_used` | Node.js, Python |
| `iii.worker.memory.heap_total` | Node.js, Python |
| `iii.worker.uptime_seconds` | Node.js, Python |
| `iii.worker.event_loop.lag_ms` | Node.js only |

---

## Graceful Shutdown

### TypeScript

```typescript
import { shutdownOtel } from 'iii-sdk/telemetry'
await shutdownOtel()
```

### Python

```python
from iii.telemetry import shutdown_otel_async
await shutdown_otel_async()
```

### Rust

Automatic on drop.

---

## Disabling Telemetry

### Via SDK Init

```typescript
const iii = registerWorker('ws://localhost:49134', {
  otel: { enabled: false },
})
```

### Via Environment Variable

```bash
OTEL_ENABLED=false
```

---

## Environment Variables

| Variable | Default | SDKs |
|----------|---------|------|
| `OTEL_ENABLED` | `true` | Node.js, Python |
| `OTEL_SERVICE_NAME` | Worker name | Node.js, Python |
| `III_URL` | `ws://localhost:49134` | Python |

---

## Engine-Side OtelModule Config

```yaml
- class: modules::observability::OtelModule
  config:
    enabled: true
    exporter: otlp                          # memory | otlp | both
    endpoint: ${OTEL_ENDPOINT:http://localhost:4317}
    service_name: iii
    service_version: 1.0.0
    sampling_ratio: 1.0                     # 0.0--1.0
    level: warn                             # trace | debug | info | warn | error
    format: json                            # default | json
    metrics_enabled: true
    metrics_exporter: memory
    logs_enabled: true
    logs_exporter: both
    logs_console_output: true
    sampling:
      default: 0.5
      parent_based: true
      rules:
        - operation: "api.*"
          rate: 1.0
      rate_limit:
        max_traces_per_second: 100
    alerts:
      - name: "High Error Rate"
        metric: "iii.invocations.errors"
        threshold: 10
        operator: ">"
        window_seconds: 60
        cooldown_seconds: 300
        action:
          type: webhook
          url: "https://alerts.example.com"
```

Prometheus metrics exposed at port **9464** (`/metrics` endpoint).

## Key Notes

- Python SDK sends telemetry directly to the iii engine over WebSocket, not to standalone OTLP endpoints.
- Enable `enableMetricsReporting: true` in SDK init to collect worker-level CPU/memory gauges.
- Use `sampling_ratio` in the OtelModule to control trace volume in production (e.g., 0.1 for 10%).
