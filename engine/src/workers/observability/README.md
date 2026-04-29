# iii-observability

Full OpenTelemetry observability for III Engine: distributed tracing, structured logs, performance metrics, alert rules, and trace sampling — all queryable via built-in functions.

## Sample Configuration

```yaml
- name: iii-observability
  config:
    enabled: true
    service_name: my-service
    service_version: 1.0.0
    exporter: memory
    metrics_enabled: true
    logs_enabled: true
    memory_max_spans: 1000
    sampling_ratio: 1.0
    alerts:
      - name: high-error-rate
        metric: iii.invocations.error
        threshold: 10
        operator: ">"
        window_seconds: 60
        action:
          type: log
```

## Configuration

| Field | Type | Description |
|---|---|---|
| `enabled` | boolean | Whether OpenTelemetry tracing export is enabled. Defaults to `false`. Env: `OTEL_ENABLED`. |
| `service_name` | string | Service name in traces and metrics. Defaults to `"iii"`. Env: `OTEL_SERVICE_NAME`. |
| `service_version` | string | Service version (`service.version` OTEL attribute). Env: `SERVICE_VERSION`. |
| `service_namespace` | string | Service namespace (`service.namespace` OTEL attribute). Env: `SERVICE_NAMESPACE`. |
| `exporter` | string | Trace exporter: `memory`, `otlp`, or `both`. Defaults to `otlp`. Env: `OTEL_EXPORTER_TYPE`. |
| `endpoint` | string | OTLP collector endpoint. Defaults to `"http://localhost:4317"`. Env: `OTEL_EXPORTER_OTLP_ENDPOINT`. |
| `sampling_ratio` | number | Global trace sampling ratio (`0.0`–`1.0`). Defaults to `1.0`. Env: `OTEL_TRACES_SAMPLER_ARG`. |
| `memory_max_spans` | number | Max spans to keep in memory. Defaults to `1000`. Env: `OTEL_MEMORY_MAX_SPANS`. |
| `metrics_enabled` | boolean | Whether metrics collection is enabled. Defaults to `false`. Env: `OTEL_METRICS_ENABLED`. |
| `metrics_exporter` | string | Metrics exporter: `memory` or `otlp`. Defaults to `memory`. Env: `OTEL_METRICS_EXPORTER`. |
| `metrics_retention_seconds` | number | How long to retain metrics in memory. Defaults to `3600`. Env: `OTEL_METRICS_RETENTION_SECONDS`. |
| `metrics_max_count` | number | Max metric data points in memory. Defaults to `10000`. Env: `OTEL_METRICS_MAX_COUNT`. |
| `logs_enabled` | boolean | Whether structured log storage is enabled. |
| `logs_exporter` | string | Logs exporter: `memory`, `otlp`, or `both`. Defaults to `memory`. Env: `OTEL_LOGS_EXPORTER`. |
| `logs_max_count` | number | Max log entries in memory. Defaults to `1000`. |
| `logs_retention_seconds` | number | How long to retain logs in memory. Defaults to `3600`. |
| `logs_sampling_ratio` | number | Fraction of logs to retain (`0.0`–`1.0`). Defaults to `1.0`. |
| `logs_console_output` | boolean | Print ingested logs to the console. Defaults to `true`. |
| `level` | string | Minimum log level: `trace`, `debug`, `info`, `warn`, `error`. Defaults to `info`. |
| `format` | string | Log output format: `default` or `json`. Defaults to `default`. |
| `alerts` | AlertRule[] | Alert rules evaluated against metrics. |

### Alert Rule Fields

| Field | Type | Description |
|---|---|---|
| `name` | string | Required. Unique alert rule name. |
| `metric` | string | Required. Metric name to monitor (e.g., `iii.invocations.error`). |
| `threshold` | number | Required. Threshold value. |
| `operator` | string | Comparison operator: `>`, `>=`, `<`, `<=`, `==`, `!=`. Defaults to `>`. |
| `window_seconds` | number | Time window in seconds for metric evaluation. Defaults to `60`. |
| `cooldown_seconds` | number | Minimum interval between alert fires. Defaults to `60`. |
| `enabled` | boolean | Whether the alert rule is active. Defaults to `true`. |
| `action` | AlertAction | `{ "type": "log" }`, `{ "type": "webhook", "url": "..." }`, or `{ "type": "function", "path": "..." }`. |

### Advanced Sampling

```yaml
sampling:
  default: 1.0
  parent_based: true
  rules:
    - operation: "api.*"
      rate: 0.1
  rate_limit:
    max_traces_per_second: 100
```

## Functions

### Logging

| Function | Description |
|---|---|
| `engine::log::info` | Log an informational message. |
| `engine::log::warn` | Log a warning message. |
| `engine::log::error` | Log an error message. |
| `engine::log::debug` | Log a debug message. |
| `engine::log::trace` | Log a trace-level message. |

All logging functions accept: `message` (string, required), `data` (object), `trace_id` (string), `span_id` (string), `service_name` (string).

### Logs API

| Function | Description |
|---|---|
| `engine::logs::list` | Query stored log entries. Filters: `start_time`, `end_time`, `trace_id`, `span_id`, `severity_min`, `severity_text`, `offset`, `limit`. |
| `engine::logs::clear` | Clear all stored log entries from memory. |

### Traces API

| Function | Description |
|---|---|
| `engine::traces::list` | List stored spans. Filters: `trace_id`, `service_name`, `name`, `status`, `min_duration_ms`, `max_duration_ms`, `start_time`, `end_time`, `sort_by`, `sort_order`, `attributes`, `include_internal`, `offset`, `limit`. |
| `engine::traces::tree` | Retrieve a trace as a hierarchical span tree. Parameters: `trace_id` (required). |
| `engine::traces::clear` | Clear all stored trace spans from memory. |

### Metrics API

| Function | Description |
|---|---|
| `engine::metrics::list` | List metrics with aggregated statistics. Returns engine counters (invocations, workers, performance), SDK metrics, and optional time-bucketed aggregations. |
| `engine::rollups::list` | List metric rollup aggregations (1-minute, 5-minute, 1-hour windows). |

### Other APIs

| Function | Description |
|---|---|
| `engine::baggage::get` | Get a baggage value from the current trace context. |
| `engine::baggage::set` | Set a baggage value in the current trace context. |
| `engine::baggage::get_all` | Get all baggage key-value pairs. |
| `engine::sampling::rules` | List all active sampling rules. |
| `engine::health::check` | Check engine health status. Returns `status`, `components`, `timestamp`, `version`. |
| `engine::alerts::list` | List all configured alert rules and their current state. |
| `engine::alerts::evaluate` | Manually trigger evaluation of all alert rules. |

## Trigger Type: `log`

Register a function to react to log entries as they are produced.

| Config Field | Type | Description |
|---|---|---|
| `level` | string | Log level to subscribe to: `info`, `warn`, `error`, `debug`, or `trace`. When omitted, fires for all levels. |

### Sample Code

```typescript
const fn = iii.registerFunction(
  'monitoring::onError',
  async (logEntry) => {
    await sendAlert({
      message: logEntry.body,
      severity: logEntry.severity_text,
      traceId: logEntry.trace_id,
    })
    return {}
  },
)

iii.registerTrigger({
  type: 'log',
  function_id: fn.id,
  config: { level: 'error' },
})
```

Log entry payload fields: `timestamp_unix_nano`, `observed_timestamp_unix_nano`, `severity_number`, `severity_text`, `body`, `attributes`, `trace_id`, `span_id`, `resource`, `service_name`, `instrumentation_scope_name`, `instrumentation_scope_version`.
