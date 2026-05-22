---
type: how-to
functions: [engine::baggage::get, engine::baggage::set, engine::baggage::get_all]
title: Read and locally set OpenTelemetry baggage
---

# When to use

The three functions inspect the OpenTelemetry **baggage** attached to the current trace context — string-keyed values that propagate alongside spans for cross-cutting concerns (tenant id, request id, A/B variant, feature flags). All three are read-side or local-only: there is no engine function that writes baggage so the next inbound request sees it.

| Question                                                          | Use this                          |
|-------------------------------------------------------------------|-----------------------------------|
| Read one baggage key                                              | `engine::baggage::get`            |
| Read every baggage key                                            | `engine::baggage::get_all`        |
| Set a value for the current call's downstream code                | `engine::baggage::set` (see warning below) |

> **Warning — `baggage::set` does not propagate.** OpenTelemetry baggage is immutable; the engine constructs a new context with the value but cannot push it back to the caller's invocation. The function exists for verification and debugging. Real baggage propagation must happen at the **SDK / invocation level** by attaching baggage headers to the next outbound call. The function returns a `note` field reminding the caller of this.

Reach for `get` / `get_all` when:

- A handler needs the tenant/user/request id the upstream caller stamped onto baggage.
- You're debugging trace correlation and want to verify baggage is flowing.

Use a function-input field instead of `baggage::set` when you actually need to pass a value to a downstream call — baggage is for opaque cross-cutting context, not for inter-function arguments.

# `engine::baggage::get`

## Inputs

```json
{
  "key": "tenant_id"     // required. The baggage key to read.
}
```

## Outputs

```json
{
  "value": "tenant-7"    // The string value, or null when the key isn't present in the current baggage.
}
```

# `engine::baggage::get_all`

## Inputs

```json
{}
```

`get_all` takes no fields.

## Outputs

```json
{
  "baggage": {
    "tenant_id":  "tenant-7",
    "request_id": "req-abc-123"
  }
}
```

The response is an object map of every baggage key/value in the current context. Empty baggage returns `{ "baggage": {} }`.

# `engine::baggage::set`

## Inputs

```json
{
  "key":   "feature_variant",   // required. Baggage key.
  "value": "experiment-A"       // required. String value to set.
}
```

## Outputs

```json
{
  "success": true,
  "note":    "Baggage set in new context. For propagation, use SDK-level baggage headers."
}
```

The `note` field is the engine's reminder that **this set is local-only**. The new context exists for the duration of the call but is not handed back to the caller's tracing pipeline. Verify, debug, or test with this function — do not rely on it for runtime context propagation.

# Worked example

Read the `tenant_id` baggage key the upstream caller stamped onto the trace context:

```json
{ "key": "tenant_id" }
```

The typical pattern is `get`/`get_all` for inspection. A handler reads `tenant_id` from baggage, branches on it, and proceeds; for tracing visibility this means the value was stamped by an upstream caller (HTTP middleware, SDK auto-instrumentation, etc.). `set` is only useful in tests or when verifying the OTel pipeline is wired up correctly.

For runnable scaffolds and the SDK-level pattern for actually propagating baggage, see the observability worker source and SDK examples in [the iii main repo](https://github.com/iii-hq/iii).

# Related

- `iii-observability` worker config — sampling and trace-context configuration governs whether baggage flows through to your handler. The relevant knobs are `sampling_ratio` (global trace ratio) and the advanced `sampling:` block (`default`, `parent_based`, `rules: [{operation, rate}]`, `rate_limit: { max_traces_per_second }`); set per `iii-config.yaml`.
- [`engine::traces::tree`](iii://iii-observability/engine/traces/traces) — when investigating a trace, `attributes` on each span often duplicate baggage values, useful as a cross-check.
