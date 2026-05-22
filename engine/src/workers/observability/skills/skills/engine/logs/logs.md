---
type: how-to
functions: [engine::logs::list, engine::logs::clear]
title: Query and clear stored log entries
---

# When to use

The two functions read and reset the in-memory OTel log store. Pick by intent:

| Question                                                    | Use this              |
|-------------------------------------------------------------|-----------------------|
| Show me logs matching a filter (time range, trace, severity)| `engine::logs::list`  |
| Wipe the in-memory log store (test setup, manual cleanup)   | `engine::logs::clear` |
| React to each new log as it arrives                          | The `log` reactive trigger — see [React to ingested log entries](iii://iii-observability/engine/log/reactive-triggers) |

Reach for `list` when:

- You're rendering an in-engine dashboard or debug UI and need recent entries.
- You're correlating spans + logs for a specific `trace_id`.
- You're inspecting why an alert fired and want the surrounding log context.

Reach for `clear` only when:

- You're running an integration test that needs a clean log store before assertions.
- An operator wants to flush memory without restarting the engine (rare).

Note: when the store fills up, the engine automatically evicts the oldest entries based on `logs_max_count` and `logs_retention_seconds` — `clear` is not needed for routine memory management.

# `engine::logs::list`

## Inputs

```json
{
  "start_time":    "2026-05-20T17:00:00Z",   // optional. RFC 3339; only entries at or after this time are returned.
  "end_time":      "2026-05-20T18:00:00Z",   // optional. RFC 3339; only entries at or before this time are returned.
  "trace_id":      "0123456789abcdef0123456789abcdef",  // optional. Restrict to a single trace.
  "span_id":       "0123456789abcdef",       // optional. Restrict to a single span; usually paired with trace_id.
  "severity_min":  9,                        // optional. OTel severity number (TRACE=1, DEBUG=5, INFO=9, WARN=13, ERROR=17). Inclusive lower bound.
  "severity_text": "ERROR",                  // optional. Exact match on the severity label; mutually useful with severity_min.
  "offset":        0,                        // optional. Pagination offset; defaults to 0.
  "limit":         100                       // optional. Max entries returned. Defaults to the worker's configured page size.
}
```

All fields are optional. With no filters, the call returns the most recent page of entries.

## Outputs

The response is a paginated list of OTel log records — same shape the `log` trigger handler receives, plus paging metadata. Each entry carries `timestamp_unix_nano`, `severity_number`, `severity_text`, `body`, `attributes`, `trace_id`, `span_id`, `resource`, `service_name`, and instrumentation scope info. See [React to ingested log entries](iii://iii-observability/engine/log/reactive-triggers) for the full record shape.

# `engine::logs::clear`

## Inputs

```json
{}
```

`clear` takes no fields.

## Outputs

The response acknowledges the wipe and reports how many entries were removed; the exact shape is implementation-defined and intended for human inspection rather than programmatic branching. The next `engine::logs::list` will return an empty list until new entries arrive.

# Worked example

Pull the most recent 100 ERROR-level entries from the last hour:

```json
{
  "start_time":    "2026-05-20T17:00:00Z",
  "end_time":      "2026-05-20T18:00:00Z",
  "severity_text": "ERROR",
  "limit":         100
}
```

The typical pattern is to scope a `list` call by the field that's most selective — `trace_id` if you're investigating one request, `severity_text: "ERROR"` for a recent-errors view, or `start_time`/`end_time` for time-bound dashboards. Pair with `engine::traces::tree` (passing the same `trace_id`) to get spans + logs for the same operation. Use `clear` only in tests or one-off operator workflows.

For runnable scaffolds, see the observability worker source and SDK examples in [the iii main repo](https://github.com/iii-hq/iii).

# Related

- [`engine::log::*`](iii://iii-observability/engine/log/log) — the emit side; entries returned by `list` are the entries `log::info`/`warn`/`error`/`debug`/`trace` produced.
- [React to ingested log entries](iii://iii-observability/engine/log/reactive-triggers) — the reactive counterpart to polling `list`.
- [`engine::traces::tree`](iii://iii-observability/engine/traces/traces) — pair with `list` filtered by `trace_id` to view both logs and spans for one trace.
