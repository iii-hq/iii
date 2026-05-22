---
type: how-to
function_id: engine::rollups::list
title: List metric rollup aggregations
---

# When to use

Call `engine::rollups::list` for pre-computed time-window aggregates of engine metrics. The engine maintains 1-minute, 5-minute, and 1-hour rollup windows so dashboards and alerting can read aggregated values without re-bucketing raw measurements on every call.

Reach for it when:

- You want a "last hour by minute" or "last day by hour" view without paying for ad-hoc `metrics::list` bucketing.
- An alert rule references a per-window aggregate; the same numbers feeding `engine::alerts::evaluate` are available here.
- You're building a long-window trend that would be too coarse with raw `metrics::list` bucketing.

Use [`engine::metrics::list`](iii://iii-observability/engine/metrics/list) instead when you want the raw current value or non-standard bucket sizes.

# Inputs

```json
{
  "names":  ["iii.invocations.total"],   // optional. Restrict to these metric names; omit to return rollups for every known metric.
  "window": "1m"                         // optional. One of "1m" | "5m" | "1h". When omitted, returns rollups for all three windows.
}
```

All fields are optional. With no filters, the call returns every metric's rollup entries across all three windows.

# Outputs

The response is keyed by metric name, with per-window arrays of bucketed aggregates. Each bucket carries a window-start timestamp, the aggregate value (sum, average, count, min/max — varies by metric type), and the bucket duration. Use the returned bucket boundaries when rendering — they're aligned to the engine's clock, not the request time.

# Worked example

Read 1-minute rollups for the invocation total — useful for a live dashboard tile:

```json
{
  "names":  ["iii.invocations.total"],
  "window": "1m"
}
```

The typical pattern is to read rollups for a specific metric at the window most useful to the consumer:

- 1-minute window for live dashboards (how busy is the engine right now?).
- 5-minute window for medium-range trends (is the error rate climbing this hour?).
- 1-hour window for capacity planning and slow-moving SLOs.

Pair with `engine::alerts::list` to see which rule operates on which rollup; the rule's `metric` and `window_seconds` map to the rollup window.

For runnable scaffolds, see the observability worker source and SDK examples in [the iii main repo](https://github.com/iii-hq/iii).

# Related

- [`engine::metrics::list`](iii://iii-observability/engine/metrics/list) — the underlying metric values that feed into these rollups.
- [`engine::alerts::list`](iii://iii-observability/engine/alerts/alerts) — alerts evaluate against rollup windows; the rule's `window_seconds` maps to one of the windows here.
