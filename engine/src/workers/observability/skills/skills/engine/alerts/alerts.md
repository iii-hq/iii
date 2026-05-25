---
type: how-to
functions: [engine::alerts::list, engine::alerts::evaluate]
title: List configured alert rules and trigger an evaluation pass
---

# When to use

The two functions cover the two operations on configured alert rules:

| Question                                                              | Use this                    |
|-----------------------------------------------------------------------|-----------------------------|
| Which alerts are configured, and which are firing right now?          | `engine::alerts::list`      |
| Force an evaluation pass against current metric values (no waiting)   | `engine::alerts::evaluate`  |

Reach for `list` when:

- An on-call dashboard needs the current state of every configured alert.
- An operator wants to verify a new alert rule from `iii-config.yaml` registered correctly.
- An incident-response tool needs to know which thresholds the engine has breached.

Reach for `evaluate` when:

- You're testing an alert rule end-to-end and don't want to wait for the next periodic evaluation.
- You're forcing a re-evaluation after manually clearing or backfilling metrics.

Alert rule definitions live in `iii-config.yaml` (`alerts:` array). Each rule has: `name` (required, unique), `metric` (required, e.g. `iii.invocations.error`), `threshold` (required, number), `operator` (`>` | `>=` | `<` | `<=` | `==` | `!=`; default `>`), `window_seconds` (default `60`), `cooldown_seconds` (default `60`), `enabled` (default `true`), and `action` — one of `{ "type": "log" }`, `{ "type": "webhook", "url": "..." }`, or `{ "type": "function", "path": "..." }`. Neither function modifies rules — they're config-driven.

# `engine::alerts::list`

## Inputs

```json
{}
```

`list` takes no fields.

## Outputs

The response is an array of configured alert rules with their current state — typical fields per rule include `name`, the configured `metric` / `threshold` / `operator` / `window_seconds`, the `enabled` flag, the last evaluation timestamp, and a current `state` (e.g. `firing` / `ok`) plus the last value seen against the threshold. Use the `name` field as the join key against alert action artifacts (logs, webhook deliveries).

# `engine::alerts::evaluate`

## Inputs

```json
{}
```

`evaluate` takes no fields. The function evaluates **every** configured rule against current metric values; per-rule evaluation cannot be requested.

## Outputs

The response summarizes the evaluation pass: how many rules were evaluated, how many transitioned state (firing→ok or ok→firing), and a per-rule breakdown matching the shape of `alerts::list` rows. Action side effects (`log`, `webhook`, `function` per the rule's `action` field) fire as part of the evaluation; the response itself is informational.

# Worked example

Both functions take no input — invoke either with an empty payload:

```json
{}
```

The typical pattern is `list` for periodic dashboard reads, `evaluate` for end-to-end verification. After editing `iii-config.yaml` and restarting, call `list` to confirm the new rule is registered, then call `evaluate` and re-`list` to confirm the rule fires when the threshold is breached. For runtime alerting, the engine evaluates rules automatically on a periodic schedule — `evaluate` is for forcing the schedule, not for routine operation.

For runnable scaffolds, see the observability worker source and SDK examples in [the iii main repo](https://github.com/iii-hq/iii).

# Related

- `iii-observability` worker config — the `alerts:` block in `iii-config.yaml` is the source of truth for rule definitions; see the schema in **When to use** above.
- [`engine::metrics::list`](iii://iii-observability/engine/metrics/list) — alert rules evaluate against the metric values returned here.
- [`engine::rollups::list`](iii://iii-observability/engine/rollups/list) — when an alert's `window_seconds` matches a rollup window (60 / 300 / 3600), the rollup is the cheapest source for the same value.
- [`engine::health::check`](iii://iii-observability/engine/health/check) — quick "is anything firing right now?" alternative.
