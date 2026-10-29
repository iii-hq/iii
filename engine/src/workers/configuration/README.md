# configuration

Schema-validated, reactive registry of named configuration entries. Workers register their configuration ids with a JSON Schema, publish values that are validated against it, and the engine calls `configuration::on-updated` on every change so other workers observe it and react without polling.

The default `fs` adapter stores one YAML file per id under a configurable directory and watches it for external edits — manual edits surface as `configuration:updated` events the same way SDK calls do. The `bridge` adapter delegates to a remote III Engine and re-broadcasts its events into the local fan-out. Reads expand `${VAR:default}` placeholders against the live process env on every call, so env changes propagate without a worker restart.

`configuration` is engine-owned. Configure it under `engine.workers.configuration` in a managed
`worker-compose.yaml`; do not add it as a project container.

## Skills

Install the `configuration` agent skill for Claude Code, Cursor, and 30+ other agents:

```bash
npx skills add iii-hq/iii --full-depth --skill configuration
```

## Sample Configuration

```yaml
engine:
  workers:
    configuration:
      adapter:
        name: fs
        config:
          directory: ./config
```

## Configuration

| Field | Type | Description |
|---|---|---|
| `adapter` | Adapter | Adapter for configuration persistence. Defaults to `fs`. |

## Adapters

### fs

File-system adapter that stores one YAML file per configuration id and watches the directory for external edits.

```yaml
name: fs
config:
  directory: ./config
```

| Field | Type | Description |
|---|---|---|
| `directory` | string | Directory holding `<id>.yaml` files. Created on boot. Defaults to `./config`. |

The default location was previously `./data/configuration`. When running on the default `./config`, the adapter performs a one-time soft migration on boot: every `<id>.yaml` still in `./data/configuration` is moved across (logged at INFO). If a file with the same name already exists in `./config`, that file is **skipped with a WARNING** and the legacy copy is left untouched for you to reconcile. An explicit `directory:` override disables the migration.

External writes / edits / removals to the watched directory are debounced (500 ms) and replayed as `configuration:registered`, `configuration:updated`, or `configuration:deleted` events through the same fan-out as SDK calls. A hand-edit that fails schema validation (after `${VAR}` expansion) is **rejected with a WARNING** — the previous good value is kept rather than loaded.

### bridge

Forwards every `configuration::*` call to a remote III Engine via the iii-sdk and observes the remote engine's `configuration::on-updated`, so changes on the source engine are mirrored into the local fan-out.

```yaml
name: bridge
config:
  bridge_url: ${REMOTE_III_URL:ws://localhost:49134}
```

| Field | Type | Description |
|---|---|---|
| `bridge_url` | string | WebSocket URL of the remote III Engine. Defaults to `ws://localhost:49134`. |

The `bridge` adapter cannot delete configurations on the remote engine; delete them by operating on the remote engine directly.

`configuration::ensure` over the `bridge` is forwarded to the remote engine's own `configuration::ensure` — the original seed candidate is sent to the remote and the seed-vs-preserve decision is made at the authoritative engine, so a stale local cache can never overwrite an operator value on the remote engine. If the remote engine does not expose `configuration::ensure` (an older engine), the call fails closed with `ADAPTER_ERROR`; the bridge deliberately does **not** fall back to a legacy `configuration::register`, which would risk overwriting remote state.

## Functions

### `configuration::register`

Declare a configuration id with a JSON Schema, name, description, and optional initial value. Idempotent — re-registering replaces the schema, name, description, and metadata; the stored value is kept unless `initial_value` is supplied. Validates `initial_value` against `schema` before persisting. Fires `configuration:registered` on first registration or `configuration:updated` on subsequent calls.

Parameters: `id` (string), `name` (string), `description` (string), `schema` (object — JSON Schema), `initial_value` (any, optional), `metadata` (any, optional)

Returns: the stored entry `{ id, name, description, schema, value, metadata }`. Templates inside `value` are stored verbatim; expansion happens on read.

### `configuration::ensure`

Atomically ensure an id exists with a schema, name, description, and metadata, seeding `initial_value` **only when no non-null value is stored yet**. If a value already exists (including `false`, `0`, or `""` — these are real values, not "empty"), it is preserved verbatim and the candidate seed is ignored and never validated. Unlike `configuration::register`, `ensure` never overwrites a stored value, so it is the race-free way for one or many workers to seed a default — use it instead of read-then-`register`. When the seed *is* applied it is validated against `schema` exactly like `register` validates `initial_value`. Fires `configuration:registered` on first creation or `configuration:updated` when an existing entry is refreshed.

`register`, `ensure`, `set`, and `delete` are linearized per store (one engine process — the `fs` adapter's on-disk store), so with the `fs` adapter a seed can never clobber a concurrent `set` and two racing seeds resolve to a single winner without a lost write. This lock guards the LOCAL store only: with the `bridge` adapter the authoritative store is the remote engine and its own linearization applies (the local cache is a best-effort mirror), so `configuration::ensure` is decided remotely (see the `bridge` adapter above), never against the local cache. External direct edits to an `fs` file are surfaced by the directory watcher and reconciled into the in-memory cache against the adapter's CURRENT authoritative state under the write path's lock — a watcher event is a hint, not a trusted snapshot. The event is captured and queued before it is applied, so a newer local `register`/`ensure`/`set`/`delete` can commit in between; the reconcile re-reads the adapter and adopts its current value, so a stale event never reverts an internal write and a stale delete never drops a locally recreated entry (a failed authoritative read keeps the last-known cache rather than deleting). This matters because the watcher echo-suppresses a superseded edit, so no later corrective event would repair a cache a trusted-verbatim snapshot had reverted. Genuine concurrent external edits remain last-writer-wins. The `bridge` adapter's authority is the remote engine's ordered event stream, so its relayed events are applied directly, without a reconciling round-trip. Fan-out happens after each mutation commits, is suppressed for an external event a newer local mutation superseded, and is not strictly ordered across concurrent callers.

Parameters: `id` (string), `name` (string), `description` (string), `schema` (object — JSON Schema), `initial_value` (any, optional — the seed candidate), `metadata` (any, optional)

Returns: `action` (one of `seeded`, `preserved`, `registered`) and `entry` (the stored `{ id, name, description, schema, value, metadata }`). `seeded` means the candidate was installed; `preserved` means an existing value was kept; `registered` means the entry was created or refreshed with a null value because no seed was supplied.

### `configuration::set`

Replace the stored value for a registered id. Validates `value` against the registered schema. Fires `configuration:updated`.

Parameters: `id` (string), `value` (any)

Returns: `old_value` (any, or `null` if the entry had no prior value), `new_value` (any)

### `configuration::get`

Read one configuration by id.

Parameters: `id` (string), `raw` (boolean, optional — defaults to `false`)

Returns: `id` (string), `value` (any). When `raw` is `false`, `${VAR:default}` placeholders inside string fields are expanded against the live process env. When `raw` is `true`, the stored value is returned verbatim.

### `configuration::list`

Enumerate every registered configuration. Never returns the stored value — pair with `configuration::get` once you have the id you want.

Returns: `configurations` (array of `{ id, name, description, schema, metadata }`), sorted lexicographically by `id`.

### `configuration::schema`

Read the schema, name, description, and metadata for one id without exposing the value.

Parameters: `id` (string)

Returns: `id` (string), `name` (string), `description` (string), `schema` (object), `metadata` (any, optional)

### Error Codes

| Code | Meaning |
|---|---|
| `NOT_REGISTERED` | `set` was called against an id that has not been passed to `register` yet. |
| `INVALID_ID` | The id does not match `[a-z0-9_-]{1,64}` — the constraint applied so ids are safe filenames for the `fs` adapter. |
| `SCHEMA_INVALID` | The supplied value does not satisfy the registered JSON Schema. The error message lists each violation. |
| `SCHEMA_UNAVAILABLE` | `set` was called for an id whose schema is not yet known — the owning worker has not registered it this session. The `fs` adapter does not persist the schema, so it is absent until a worker re-registers after boot. |
| `NOT_FOUND` | `get` or `schema` was called against an id that is not registered. |
| `ADAPTER_ERROR` | The adapter failed to persist the change (disk error, bridge unreachable, etc.). |

## Engine-hosted source: `configuration::on-updated`

The engine calls `configuration::on-updated` when a configuration entry is registered, updated, or deleted — including external `fs` file edits and bridge-forwarded events from a remote engine — with the event as its input and output. It is a private function: only the engine calls it. Observe it to react to changes; filter on `input.id` and `input.event_type` to pick them.

### Configuration Event Payload

| Field | Type | Description |
|---|---|---|
| `type` | string | Always `"configuration"`. |
| `event_type` | string | `"configuration:registered"`, `"configuration:updated"`, or `"configuration:deleted"`. |
| `id` | string | The configuration id that changed. |
| `name` | string | The registered name at the time of the event. |
| `description` | string | The registered description. |
| `schema` | object | The registered JSON Schema. |
| `old_value` | any | Previous value with `${VAR:default}` placeholders expanded. `null` on `configuration:registered`. |
| `new_value` | any | New value with `${VAR:default}` placeholders expanded. `null` on `configuration:deleted`. |
| `metadata` | any | Echoes the registered metadata; omitted when none was supplied. |

### Sample Code

```typescript
iii.registerFunction('stream::on-config-change', async ({ input: event }) => {
  console.log('Configuration changed:', event.event_type, event.id)
  console.log('New value:', event.new_value)
  return {}
})

iii.observe(
  {
    function_id: 'configuration::on-updated',
    filters: {
      'input.id': { $eq: 'iii-stream' },
      'input.event_type': { $eq: 'configuration:updated' },
    },
  },
  'stream::on-config-change',
)
```

Mutations that call it: `configuration::register` (`:registered` on first call, `:updated` on re-registration), `configuration::ensure` (`:registered` when it creates an entry, `:updated` when it refreshes one), `configuration::set` (`:updated`), and external `fs` create / edit / delete events. Reads (`configuration::get`, `configuration::list`, `configuration::schema`) do **not** call it.

## Usage Example: Migrating a Worker Off `config.yaml`

```typescript
import { registerWorker } from 'iii-sdk'

const iii = registerWorker('ws://localhost:49134')

// 1. Declare the schema once at startup.
await iii.call({
  function_id: 'configuration::register',
  payload: {
    id: 'iii-stream',
    name: 'Stream worker',
    description: 'Connection settings for the stream worker.',
    schema: {
      type: 'object',
      required: ['port'],
      properties: {
        port: { type: 'integer', minimum: 1, maximum: 65535 },
        host: { type: 'string' },
      },
    },
    initial_value: { port: 3112, host: '127.0.0.1' },
  },
})

// 2. Read the live value (placeholders expanded) anywhere you need it.
const { value } = await iii.call({
  function_id: 'configuration::get',
  payload: { id: 'iii-stream' },
})

// 3. Observe changes elsewhere — set/external-edit/bridge events all
//    arrive through the same source.
iii.registerFunction('stream::on-config-change', async ({ input: event }) => {
  console.log('Stream config now:', event.new_value)
  return {}
})

iii.observe(
  { function_id: 'configuration::on-updated', filters: { 'input.id': { $eq: 'iii-stream' } } },
  'stream::on-config-change',
)
```
