---
type: how-to
function_id: engine::metrics::list
title: List engine and SDK metrics with aggregated statistics
---

# When to use

Call `engine::metrics::list` to inspect counter, gauge, and histogram values the engine and SDK have recorded. The function aggregates raw measurements into named metrics and returns each one's current value plus optional time-bucketed series. It's the right call for an at-a-glance health view, a CLI snapshot, or building a custom dashboard against the engine's in-memory metric store.

Reach for it when:

- You want the current invocation count, error count, latency stats, queue depths, etc. without hooking up an OTLP backend.
- You need to compare two points in time without a separate metrics database — pass a time range and the response is bucketed.
- You're verifying that custom SDK metrics emitted by your worker actually reached the engine.

Use [`engine::rollups::list`](iii://iii-observability/engine/rollups/list) instead when you want pre-computed window aggregates (1-minute, 5-minute, 1-hour) rather than ad-hoc bucketing.

# Inputs

```json
{
  "names":         ["iii.invocations.total", "iii.invocations.error"],  // optional. Restrict to these metric names; omit to return all.
  "start_time":    "2026-05-20T17:00:00Z",       // optional. RFC 3339; only data points at or after this time.
  "end_time":      "2026-05-20T18:00:00Z",       // optional. RFC 3339; only data points at or before this time.
  "bucket_seconds": 60                            // optional. When set, return time-bucketed values at this resolution; when omitted, only summary stats are returned.
}
```

All fields are optional. With no filters, the call returns every known metric with cumulative stats over the configured retention window (`metrics_retention_seconds`).

# Outputs

The response carries per-metric entries; each entry includes the metric name, type (`counter`/`gauge`/`histogram`), current value, summary stats (count/sum/min/max/avg, plus percentiles for histograms), and — when `bucket_seconds` was set — a `series` array of timestamped values at that resolution. Engine-emitted metric names are stable (`iii.invocations.total`, `iii.invocations.error`, `iii.workers.connected`, etc.); SDK and user metric names follow whatever the worker published.

# Worked example

Bucket the invocation total + error count for the last hour at one-minute resolution:

```json
{
  "names":          ["iii.invocations.total", "iii.invocations.error"],
  "start_time":     "2026-05-20T17:00:00Z",
  "end_time":       "2026-05-20T18:00:00Z",
  "bucket_seconds": 60
}
```

The typical pattern is to filter by `names` (so the response stays small) and pass `bucket_seconds` only when you need time series. For a dashboard tile, request the specific metric and a one-minute bucket; for a one-shot snapshot, omit `bucket_seconds` and read the summary fields. Engine metric names are documented in the iii main repo source — searching for the metric name in `engine/src/workers/observability/` shows where it's emitted.

For runnable scaffolds, see the observability worker source and SDK examples in [the iii main repo](https://github.com/iii-hq/iii).

# Related

- [`engine::rollups::list`](iii://iii-observability/engine/rollups/list) — pre-computed window aggregates; cheaper than calling `metrics::list` with `bucket_seconds`.
- [`engine::traces::list`](iii://iii-observability/engine/traces/traces) — drill from an aggregate metric into the individual spans contributing to it.
- [`engine::alerts::list`](iii://iii-observability/engine/alerts/alerts) — alert rules are evaluated against these same metrics.
