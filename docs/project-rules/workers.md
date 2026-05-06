# Workers rules

Rules for routing worker-related content and the worker concept itself.

## Worker-specific content goes to Worker Docs

Specific iii workers (state, queue, stream, cron, pubsub, http, observability, bridge, exec, worker-manager, etc.) have their own dedicated **Worker Docs** outside the iii docs.

When source content covers a specific worker — its config schema, its registered functions, its trigger types, its protocol details, its error codes — flag it for **Move to Worker Docs**. We never actually move it; that's left to the Worker Docs authors. Just note it in the decisions log.

This rule applies to:
- Per-worker config schemas (port, host, adapter, exporter, etc.).
- Functions a worker registers (e.g., `state::set`, `iii::durable::publish`, `stream::list`).
- Trigger types a worker provides (e.g., `http`, `cron`, `durable:subscriber`, `state`, `stream:join`).
- Worker-internal protocol or error codes.

## No "external vs built-in" worker distinction

A worker is a worker. Don't introduce conceptual splits between SDK-driven processes and engine-bundled workers in the iii docs. If source content draws that distinction, drop or flatten it.

## Channels belong to iii-worker-manager

The whole channel surface — concept, lifecycle, writer/reader/refs, examples — belongs in the iii-worker-manager Worker Docs.

In the iii docs:
- Strip channel-related types (`Channel`, `ChannelReader`, `ChannelWriter`, `ChannelDirection`, `StreamChannelRef`) from SDK reference pages.
- Add a callout on each SDK page pointing readers to iii-worker-manager. See [`sdks.md`](./sdks.md) for the callout convention.

## Logger and telemetry belong to iii-observability

Logger methods, OpenTelemetry init, custom spans, worker metrics, log emission and subscription — all observability surface is documented with the iii-observability worker.

In the iii docs:
- Strip these surfaces from SDK reference pages.
- Drop telemetry-related SDK types (`OtelConfig`, OTel-specific `ReconnectionConfig`, `TelemetryOptions`).
- Add a callout on each SDK page pointing readers to iii-observability. See [`sdks.md`](./sdks.md) for the callout convention.

**`iii-observability` and `iii-telemetry` are separate workers.** `iii-observability` owns OpenTelemetry (traces, metrics, logs, baggage, sampling, alerts) — that's the surface SDK callouts point at. `iii-telemetry` is the anonymous-usage worker (Amplitude analytics, heartbeat, device-ID management) and is governed by the `III_TELEMETRY_ENABLED` env var, not `OTEL_ENABLED`. Don't conflate them — they have separate Worker Docs targets, and `how-to/disable-telemetry.mdx` documents both env vars distinctly.

## Worker authoring is outside the iii docs

Implementing engine traits, building a custom worker package, designing per-language worker scaffolds — all worker-authoring content belongs outside the iii docs (Worker Docs authoring guides, not user-facing iii docs).

## Surface broadly-important Worker Docs content via callouts in iii docs

When a Worker Docs surface is broadly important to users — frequent use, agent-skill-critical, or
cross-cutting — add a brief callout in the relevant iii docs page pointing to the Worker Docs.
Don't duplicate the content; just signpost.

This generalizes the existing SDK callout pattern (logger/telemetry → iii-observability;
channels → iii-worker-manager). Other surfaces that meet this bar:

- **Introspection** (listing workers / functions / triggers; querying traces / logs / metrics) →
  callouts on `understanding-iii/engine.mdx`, `sdk-reference/engine-sdk.mdx`,
  `using-iii/console.mdx`, and `using-iii/workers.mdx` pointing at iii-engine-functions and
  iii-observability Worker Docs.

When in doubt, prefer a callout over importing the content. The Worker Docs is the canonical
home; iii docs is the signpost layer.

## `iii-http` and `iii-http-functions` are separate workers

Both exist in the engine and both are Worker Docs targets:
- `iii-http` (registered in `workers/rest_api/`, default-on) — the engine's HTTP server / REST API surface; owns the `http` trigger type and HTTP-exposed endpoints.
- `iii-http-functions` (registered in `workers/http_functions/`, default-off) — the HTTP-invocation surface (calling external HTTP services from inside iii functions).

Don't conflate them in stubs or callouts. The names are close enough to be confusing — flag for engineering naming review (see `PROGRESS-repo-checks.md` hanging pieces).
