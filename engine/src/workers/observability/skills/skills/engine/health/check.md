---
type: how-to
function_id: engine::health::check
title: Check engine health status
---

# When to use

Call `engine::health::check` to get a structured snapshot of the engine's health: an overall status, per-component detail, the engine version, and a server-side timestamp. It's the canonical "is the engine OK right now?" call — use it for liveness/readiness probes, on-call dashboards, deploy-time canaries, and operator CLIs.

Reach for it when:

- A platform-level component (load balancer, orchestrator, deploy script) needs a yes/no signal before routing traffic to this engine.
- An on-call dashboard needs a per-component breakdown to point at the failing subsystem.
- A canary script needs a deterministic success/failure check after a rolling deploy.

Use [`engine::metrics::list`](iii://iii-observability/engine/metrics/list) instead when you need quantitative load/error data rather than a status; use [`engine::alerts::list`](iii://iii-observability/engine/alerts/alerts) when you want to know whether any configured alert is currently firing.

# Inputs

```json
{}
```

`check` takes no fields.

# Outputs

```json
{
  "status":    "ok",                              // "ok" | "degraded" | "unhealthy" — the rolled-up overall status.
  "version":   "0.12.0",                          // engine version string.
  "timestamp": "2026-05-20T17:00:00.123Z",        // server-side time of the check, RFC 3339.
  "components": {                                 // per-component breakdown; keys are component names, values carry status + diagnostic detail.
    "engine":      { "status": "ok",       "details": null },
    "scheduler":   { "status": "ok",       "details": null },
    "telemetry":   { "status": "ok",       "details": null },
    "adapters":    { "status": "degraded", "details": "redis: connection refused" }
  }
}
```

- `status` is rolled up from the worst component status: any `unhealthy` makes the overall `unhealthy`; any `degraded` (with no `unhealthy`) makes the overall `degraded`; everything `ok` makes the overall `ok`.
- `components` keys depend on what's configured (adapter set, optional features). Treat unknown keys as additive — a parser should iterate rather than hard-code an enum.
- `details` is human-readable diagnostic text; expect it to be `null` on healthy components.

# Worked example

`check` takes no input — invoke with an empty payload:

```json
{}
```

The typical pattern is to read `status` for the binary signal and walk `components` only when `status !== "ok"`. For liveness probes, treat `unhealthy` as failure; for readiness, treat both `unhealthy` and `degraded` as failure (the engine should not receive traffic if any component is degraded).

For runnable scaffolds, see the observability worker source and SDK examples in [the iii main repo](https://github.com/iii-hq/iii).

# Related

- [`engine::alerts::list`](iii://iii-observability/engine/alerts/alerts) — `health::check` reports current state; `alerts::list` reports rule-based threshold breaches over a time window. Read both during incident triage.
- [`engine::metrics::list`](iii://iii-observability/engine/metrics/list) — quantitative drill-down when health is degraded.
