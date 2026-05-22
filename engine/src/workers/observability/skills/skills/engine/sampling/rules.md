---
type: how-to
function_id: engine::sampling::rules
title: List the active OpenTelemetry sampling rules
---

# When to use

Call `engine::sampling::rules` to inspect the sampling configuration currently in effect — the global default rate, the parent-based flag, per-operation rules, and any rate-limit cap. It's a read-only snapshot of the sampling policy the engine is using to decide which traces are kept.

Reach for it when:

- An operator needs to verify a recent config change actually took effect.
- A trace you expect to see is missing from `engine::traces::list` and you want to confirm whether sampling dropped it.
- You're auditing how aggressively the engine is sampling each operation type before raising or lowering the rate.

There is **no runtime sampling-mutation function**. Changing the sampling configuration is a `iii-config.yaml` edit + engine restart. The relevant fields under the worker's `sampling:` block are `default` (default ratio, `0.0`–`1.0`), `parent_based` (boolean — when true, child spans inherit the parent's decision), `rules: [{operation, rate}]` (per-operation overrides; first match wins), and `rate_limit: { max_traces_per_second }` (hard cap regardless of per-operation rate).

# Inputs

```json
{}
```

`rules` takes no fields.

# Outputs

The response describes the active sampling policy:

```json
{
  "default":            1.0,                                // the default sampling ratio (0.0 - 1.0)
  "parent_based":       true,                                // when true, child spans inherit the parent's sampling decision
  "rules": [                                                // optional per-operation overrides
    { "operation": "api.*",   "rate": 0.1 },
    { "operation": "health.*", "rate": 0.0 }
  ],
  "rate_limit": {                                            // optional global cap
    "max_traces_per_second": 100
  }
}
```

- `default` is the global ratio used when no rule matches.
- `parent_based` controls whether child spans inherit their parent span's sampling decision.
- `rules` is the per-operation override list — the first matching rule wins.
- `rate_limit` enforces a hard cap regardless of the per-operation rate.

Empty `rules` and unset `rate_limit` indicate no overrides — the `default` ratio applies to every trace.

# Worked example

`rules` takes no input — invoke with an empty payload to read the current sampling policy:

```json
{}
```

The typical pattern is a quick verification step inside an admin console or a debug handler: "given the engine's current sampling, would I see this trace?" Read the rules, find the rule whose `operation` pattern matches the operation you're investigating, and use that `rate`; if none match, use `default`.

For changing the policy, edit `iii-config.yaml` (`sampling:` block) and restart the engine. The schema is `default` (number, `0.0`–`1.0`), `parent_based` (boolean), `rules: [{operation, rate}]` (operation-pattern matched in order), and `rate_limit: { max_traces_per_second }` (a global cap that applies on top of per-rule rates).

# Related

- `iii-observability` worker config — the source of truth this function exposes at runtime; the `sampling:` block schema is documented inline above.
- [`engine::traces::list`](iii://iii-observability/engine/traces/traces) — the data the sampling policy gates; missing traces here often correlate with low sampling rates here.
