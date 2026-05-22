---
type: how-to
functions: [engine::traces::list, engine::traces::tree, engine::traces::clear]
title: Query and clear stored OpenTelemetry spans
---

# When to use

The three functions cover the three ways an agent inspects stored span data:

| Question                                                          | Use this                  |
|-------------------------------------------------------------------|---------------------------|
| Find spans matching filters (service, name, duration, time range) | `engine::traces::list`    |
| Show all spans for one `trace_id` as a parent/child tree           | `engine::traces::tree`    |
| Wipe the in-memory span store                                     | `engine::traces::clear`   |

Reach for `list` for ad-hoc search over many traces; reach for `tree` once you have a `trace_id` and want the full request waterfall; reach for `clear` only in tests or operator one-offs.

Use [`engine::logs::list`](iii://iii-observability/engine/logs/logs) instead when you want unstructured-message telemetry rather than span-shaped timing data. Pair with `traces::tree` (passing the same `trace_id`) to render logs alongside spans.

# `engine::traces::list`

## Inputs

```json
{
  "trace_id":          "0123456789abcdef0123456789abcdef",  // optional. Restrict to spans in this trace.
  "service_name":      "billing",                // optional. Restrict to one service (the resource attribute service.name).
  "name":              "process_payment",        // optional. Span name; partial-match support is implementation-defined.
  "status":            "error",                  // optional. "ok" | "error" | "unset".
  "min_duration_ms":   100,                      // optional. Inclusive lower bound on span duration.
  "max_duration_ms":   10000,                    // optional. Inclusive upper bound on span duration.
  "start_time":        "2026-05-20T17:00:00Z",   // optional. RFC 3339; only spans starting at or after this time.
  "end_time":          "2026-05-20T18:00:00Z",   // optional. RFC 3339; only spans starting at or before this time.
  "sort_by":           "start_time",             // optional. "start_time" | "duration_ms" | "name".
  "sort_order":        "desc",                   // optional. "asc" | "desc". Defaults to "desc".
  "attributes":        { "http.status_code": "500" },  // optional. Map of attribute keys to required values; spans must match every key.
  "include_internal":  false,                    // optional. When false (default), excludes spans with kind "INTERNAL" (engine plumbing).
  "offset":            0,                        // optional. Pagination offset.
  "limit":             100                       // optional. Max spans returned per page.
}
```

All fields are optional. With no filters, the call returns the most recent page of spans (excluding INTERNAL spans by default — set `include_internal: true` to see engine plumbing).

## Outputs

A paginated list of spans; each span carries `trace_id`, `span_id`, `parent_span_id` (or null for root), `name`, `kind`, `service_name`, `start_time`, `end_time`, `duration_ms`, `status`, `attributes`, and event/link arrays. The exact field set tracks the OTel span schema.

# `engine::traces::tree`

## Inputs

```json
{
  "trace_id": "0123456789abcdef0123456789abcdef"   // required. The trace id to assemble.
}
```

`trace_id` is required.

## Outputs

A single tree-shaped object: the root span at the top, each `children` array containing direct child spans (each with their own `children`), recursively. Each node has the same fields as a `traces::list` row. Empty `children: []` arrays mark leaves.

# `engine::traces::clear`

## Inputs

```json
{}
```

`clear` takes no fields.

## Outputs

The response acknowledges the wipe; the next `traces::list` returns an empty list until new spans arrive.

# Worked example

Find recent error traces in the billing service:

```json
{
  "service_name": "billing",
  "status":       "error",
  "start_time":   "2026-05-20T17:00:00Z",
  "limit":        50
}
```

Then pick a `trace_id` from the response and walk it as a parent/child tree via `engine::traces::tree`:

```json
{ "trace_id": "0123456789abcdef0123456789abcdef" }
```

The typical pattern is "find a trace, then walk it" — `list` to narrow, `tree` to drill in. Optionally call `engine::logs::list` with the same `trace_id` to interleave logs with spans. `clear` is for test setup only — production span retention is governed by `memory_max_spans`.

For runnable scaffolds, see the observability worker source and SDK examples in [the iii main repo](https://github.com/iii-hq/iii).

# Related

- [`engine::logs::list`](iii://iii-observability/engine/logs/logs) — the matching log query; pair with `traces::tree` filtered by `trace_id`.
- [`engine::log::*`](iii://iii-observability/engine/log/log) — when emitting logs from inside a span, pass `trace_id`/`span_id` so the entries link up here.
- [`engine::metrics::list`](iii://iii-observability/engine/metrics/list) — for aggregated views over many traces.
