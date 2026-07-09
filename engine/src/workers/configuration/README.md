# configuration

Schema-validated, reactive registry of named configuration entries. Workers register their configuration ids with a JSON Schema, publish values that are validated against it, and emit `configuration` triggers on every change so other workers react without polling.

The default `fs` adapter stores one YAML file per id under a configurable directory and watches it for external edits — manual edits surface as `configuration:updated` events the same way SDK calls do. The `bridge` adapter delegates to a remote III Engine and re-broadcasts its events into the local fan-out. Reads expand `${VAR:default}` placeholders against the live process env on every call, so env changes propagate without a worker restart.

## Install

```bash
iii worker add configuration
```

Resolves from the worker registry at [workers.iii.dev](https://workers.iii.dev/).

## Skills

Install the `configuration` agent skill for Claude Code, Cursor, and 30+ other agents:

```bash
npx skills add iii-hq/iii --full-depth --skill configuration
```

## Sample Configuration

```yaml
- name: configuration
  config:
    adapter:
      name: fs
      config:
        directory: ./config
    ttl_seconds: 0
```

## Configuration

| Field | Type | Description |
|---|---|---|
| `adapter` | Adapter | Adapter for configuration persistence. Defaults to `fs`. |
| `ttl_seconds` | integer | Per-id cleanup countdown in seconds. When `>0`, an entry whose last subscriber trigger has been unregistered for this long is deleted. Defaults to `0` (no cleanup). |

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
| `directory` | string | Directory holding `<id>.yaml` files. Created on boot. Defaults to `./config` (`./data/configuration` when `III_EXECUTION_CONTEXT=docker`). |

In Docker (`III_EXECUTION_CONTEXT=docker`, set by the official image) the default is `./data/configuration` instead: the Docker template mounts a persistent volume at `./data`, and the engine's non-root user cannot create `./config` in the root-owned working directory — nor would entries there survive a container recreate.

The default location was previously `./data/configuration`. When running on the default `./config`, the adapter performs a one-time soft migration on boot: every `<id>.yaml` still in `./data/configuration` is moved across (logged at INFO). If a file with the same name already exists in `./config`, that file is **skipped with a WARNING** and the legacy copy is left untouched for you to reconcile. An explicit `directory:` override disables the migration.

External writes / edits / removals to the watched directory are debounced (500 ms) and replayed as `configuration:registered`, `configuration:updated`, or `configuration:deleted` events through the same trigger fan-out used by SDK calls. A hand-edit that fails schema validation (after `${VAR}` expansion) is **rejected with a WARNING** — the previous good value is kept rather than loaded.

### bridge

Forwards every `configuration::*` call to a remote III Engine via the iii-sdk and registers a remote `configuration` trigger so changes on the source engine are mirrored into the local trigger fan-out.

```yaml
name: bridge
config:
  bridge_url: ${REMOTE_III_URL:ws://localhost:49134}
```

| Field | Type | Description |
|---|---|---|
| `bridge_url` | string | WebSocket URL of the remote III Engine. Defaults to `ws://localhost:49134`. |

The `bridge` adapter cannot delete configurations on the remote engine; cleanup happens via the remote's TTL or by operating on the remote engine directly.

## Functions

### `configuration::register`

Declare a configuration id with a JSON Schema, name, description, and optional initial value. Idempotent — re-registering replaces the schema, name, description, and metadata; the stored value is kept unless `initial_value` is supplied. Validates `initial_value` against `schema` before persisting. Fires `configuration:registered` on first registration or `configuration:updated` on subsequent calls.

Parameters: `id` (string), `name` (string), `description` (string), `schema` (object — JSON Schema), `initial_value` (any, optional), `metadata` (any, optional)

Returns: the stored entry `{ id, name, description, schema, value, metadata }`. Templates inside `value` are stored verbatim; expansion happens on read.

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

## Trigger Type: `configuration`

Fires when a configuration entry is registered, updated, or deleted — including external `fs` file edits and bridge-forwarded events from a remote engine.

| Config Field | Type | Description |
|---|---|---|
| `configuration_id` | string | Only fire for changes to this id. When omitted, fires for every id. |
| `event_types` | string[] | Subset of `configuration:registered`, `configuration:updated`, `configuration:deleted`. When omitted, fires for every event type. |
| `condition_function_id` | string | Function ID for conditional execution. If it returns `false`, the handler is skipped. |

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
const fn = iii.registerFunction(
  { id: 'stream::onConfigChange' },
  async (event) => {
    console.log('Configuration changed:', event.event_type, event.id)
    console.log('New value:', event.new_value)
    return {}
  },
)

iii.registerTrigger({
  type: 'configuration',
  function_id: fn.id,
  config: {
    configuration_id: 'iii-stream',
    event_types: ['configuration:updated'],
  },
})
```

Mutations that fire triggers: `configuration::register` (`:registered` on first call, `:updated` on re-registration), `configuration::set` (`:updated`), TTL-driven cleanup (`:deleted`), and external `fs` create / edit / delete events. Reads (`configuration::get`, `configuration::list`, `configuration::schema`) do **not** fire triggers.

## TTL Cleanup

The worker tracks the number of triggers currently bound to each configuration id. When `ttl_seconds > 0` is configured AND the **last** trigger for an id is unregistered, a countdown deletes the entry after the TTL elapses. A new trigger registration before the countdown fires aborts the cleanup; while at least one trigger is bound to an id, that entry never expires.

`ttl_seconds = 0` (the default) disables expiry entirely — entries persist until they are explicitly deleted via the adapter or, for the `fs` adapter, by removing the underlying file.

This is the cleanup story for ephemeral workers that come and go (sandboxes, short-lived consumers) and should not leave stale configuration behind when no one is left subscribing.

## Usage Example: Migrating a Worker Off `config.yaml`

```typescript
import { registerWorker } from 'iii-sdk'

const iii = registerWorker('ws://localhost:49134')

// 1. Declare the schema once at startup.
await iii.trigger({
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
const { value } = await iii.trigger({
  function_id: 'configuration::get',
  payload: { id: 'iii-stream' },
})

// 3. Subscribe to changes elsewhere — set/external-edit/bridge events all
//    arrive on the same trigger.
const onChange = iii.registerFunction('stream::onConfigChange', async (event) => {
  console.log('Stream config now:', event.new_value)
  return {}
})

iii.registerTrigger({
  type: 'configuration',
  function_id: onChange.id,
  config: { configuration_id: 'iii-stream' },
})
```
