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

Logger methods, OpenTelemetry init, custom spans, worker metrics, log emission and subscription — all observability surface lives with the iii-observability worker.

In the iii docs:
- Strip these surfaces from SDK reference pages.
- Drop telemetry-related SDK types (`OtelConfig`, OTel-specific `ReconnectionConfig`, `TelemetryOptions`).
- Add a callout on each SDK page pointing readers to iii-observability. See [`sdks.md`](./sdks.md) for the callout convention.

## Worker authoring is outside the iii docs

Implementing engine traits, building a custom worker package, designing per-language worker scaffolds — all worker-authoring content belongs outside the iii docs (Worker Docs authoring guides, not user-facing iii docs).
