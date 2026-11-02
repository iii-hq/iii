---
name: configuration
description: >-
  Schema-validated, reactive registry for named configuration entries — the
  runtime configuration surface used by engine and Compose workers.
---

# configuration

The `configuration` worker is a server-side registry of named entries. Every entry has an id (e.g. `iii-stream`, `billing-service`), a human-readable name and description, a JSON Schema describing the value shape, and a JSON value validated against that schema. Workers call `configuration::register` once at startup to declare their schema and `configuration::set` to publish values; consumers call `configuration::get` / `configuration::list` to read and observe `configuration::on-updated` to react to changes without polling.

The default `fs` adapter persists one YAML file per id under `./config` and watches the directory for external edits, so manual edits surface as `configuration:updated` events. The `bridge` adapter delegates to a remote engine. The worker is engine-owned and may be configured at `engine.workers.configuration`.

## When to Use

- A worker needs a typed, observable configuration surface other workers can read and validate against.
- Two workers need to agree on the same configuration values without one polling the other or hardcoding a path on disk.
- An operator should be able to edit a single YAML file (or the remote control plane) and have the change propagate to every subscriber without a worker restart.

## Boundaries

- Not a general-purpose key/value store — every entry must have a registered JSON Schema. Use the standalone `state` worker for free-form values.
- No partial-update surface; `set` always replaces the whole value. Build the new value client-side and ship it in one call.
- Entries persist until their file is removed from the `fs` adapter's directory; no function deletes an entry. The `bridge` adapter cannot delete entries on the remote engine; remove them on the source engine.
- Over the `bridge` adapter, `configuration::ensure` is decided by the remote engine (the original candidate is forwarded there), so it requires a remote engine that exposes `configuration::ensure`; against an older remote it fails closed with `ADAPTER_ERROR` rather than falling back to an unsafe `register`.
- Schemas are not version-checked across re-registrations — re-registering with an incompatible schema simply replaces it. Coordinate schema migrations out-of-band.

## Migration and runtime overrides

Compose defaults to `<namespace>-<container-key>` without a hash (for example,
`default-harness`). Invalid or over-64-character generated names require explicit `config_name`.
Compose migrates only the exact previous namespace/key hash before starting a stopped worker.
After hashed migration, only the `default` namespace adopts the exact bare container key,
unless another container explicitly owns it. The bare source wins over an existing destination;
`configuration::migrate` is the single operation, delegated through bridges to the authority.
It returns `{ action, entry }` with `migrated`, `preserved` (existing destination with absent
source, or matching IDs with an existing entry), or `missing` (both source and destination absent).
Before migration, Compose and every bridge hop query the read-only
`configuration::migration-capabilities` and require `source_priority_archive_revision: 1`.
Unknown or unavailable capabilities fail closed before invoking migration; upgrade the authority.
It replaces the destination with the raw source entry, then archives the original source as
`<source>.yaml.bak`. The previous destination is not backed up. `.yaml.bak`, `.bak.yaml`, and
`.bkup.yaml` files are ignored during loading, watching and legacy directory migration.
An existing identical backup permits cleanup recovery; a conflicting backup is never overwritten.
Missing source is a no-write no-op. Unsupported adapters fail closed.
Stop source consumers before migrating; do not share an fs directory between engine processes.

`config_override` is delivered through the existing service API. Compose reads the
current configuration with `configuration::get` (`raw: true`), merges defaults and
overrides, then calls `configuration::set` with `flush: false` before starting the
worker. No snapshot file is delivered. `III_CONFIG_NAME` identifies the entry.
Memory-only updates never write persistent `config/<id>.yaml`, and no delayed flush
is scheduled. `ensure` and metadata registration preserve the active value without
persisting it or notifying base defaults. An explicit `set` with default `flush: true`
persists the complete submitted object, including override values if submitted.
Both forms of `set` notify consumers; applying changes is the worker's responsibility.
A new worker start merges the current GET value. Removing an override keeps the
current value, not the old disk value. Stopping a worker does not clear active memory;
restarting the configuration service loses unsaved values and reloads its adapter.
Distinct concurrent overrides sharing one configuration id are not supported.

## Functions

- `configuration::register` — declare an id with name, description, JSON Schema, and an optional `initial_value`; idempotent re-registration replaces the schema and metadata.
- `configuration::ensure` — atomically seed a default: create or refresh the id but write `initial_value` **only when no non-null value is stored yet**; an existing value (including `false`/`0`/`""`) is preserved verbatim and the seed is ignored. The race-free replacement for read-then-`register` when seeding from one or many workers. Fires `configuration:registered` on creation or `configuration:updated` on refresh, except while an execution injection is active: boot metadata must not deliver the base in place of the active value.
- `configuration::migrate` — move an exact source id with source priority, preserving raw persisted values and metadata and archiving the source. On a successful move, source active memory follows the new id without being saved and replaces any destination override; the old id is no longer readable. With no persisted source, destination memory is preserved and orphan source memory is retired. Same-id migration preserves memory; failures retain it for retry. The response contains the persisted entry, while source `:deleted` and destination `:registered` events carry the active snapshot captured at commit. No-ops emit nothing.
- `configuration::set` — replace the complete active value and emit `configuration:updated`. `flush` defaults to true, validating and persisting through the adapter for an already-registered id. `flush: false` changes memory only and may precede registration; validation uses the schema when available and normal GET validates again after registration. Explicit null is a value, not a request to clear memory.
- `configuration::get` — read the current active entry by id; expands `${VAR:default}` against live env unless `raw: true`.
- `configuration::list` — enumerate every registered id with name, description, and schema; never returns the value.
- `configuration::schema` — read schema/name/description for one id without exposing the value.

`register`, `ensure`, `migrate`, and `set` are the mutators; the read-side functions are cache-backed and cheap. Every mutator (plus `delete`) is linearized per store (the local `fs` store, one engine process), so a seed can never clobber a concurrent `set` and two racing seeds resolve to a single winner. Over the `bridge` adapter the authoritative store is the remote engine and its own linearization applies (the local cache is a best-effort mirror), so `ensure` is decided remotely, never against the local cache. Reads expand `${VAR:default}` placeholders against the live process env on every call, so env changes propagate without restarts — pass `raw: true` to `configuration::get` when you need the stored template form.

## Reacting to changes

Observe `configuration::on-updated` when a function should run automatically on every register / set / delete — including external `fs` file edits and bridge-forwarded events from a remote engine. The engine calls it after each successful mutation and runs its observers' targets asynchronously, so a worker stays in sync with its configuration without polling.

Reach for it when:

- A worker needs to reload in-memory state when its configuration is rewritten by another component or by an operator editing the YAML directly.
- The same handler should run regardless of who edited the configuration (local SDK call, remote engine via the bridge adapter, or a file edit).

If you only need the new value inside the same function that wrote it, `configuration::set` already returns `old_value` / `new_value` — observe only when a *different* worker should react.

### How to observe

1. Register a handler: `iii.registerFunction('stream::on-config-change', handler)`.
2. Observe the source:

```typescript
iii.observe(
  {
    function_id: 'configuration::on-updated',
    filters: {
      'input.id': { $eq: 'iii-stream' },                         // optional. Omit to receive every id.
      'input.event_type': { $eq: 'configuration:updated' },      // optional. configuration:registered|updated|deleted.
    },
  },
  'stream::on-config-change',
)
```

Mutations that call it: `configuration::register` (`:registered` on first call, `:updated` on re-registration), `configuration::ensure` (`:registered` on creation, `:updated` on refresh), `configuration::set` (`:updated`), and external `fs` create/edit/delete events. Reads do **not** call it.

For the event shape, call `engine::functions::info { function_id: "configuration::on-updated" }`: its `request_format` is the event.
