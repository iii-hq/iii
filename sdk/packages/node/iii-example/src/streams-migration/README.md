# Migrating from iii-stream: runnable recipes

The iii-stream worker (its `set` / `get` / `delete` / `list` / `list_groups` / `list_all` / `send` / `update` functions, the `stream`, `stream:join` and `stream:leave` trigger types, and the dedicated Streams WebSocket) is deprecated (iii-stream) and will be removed in a future release (version TBD). Behavior is unchanged for now. Migration guide: https://iii.dev/docs/upgrading/migrate-from-streams

These three recipes rebuild what applications used iii-stream for with SDK primitives that already exist: function registration, a worker-owned trigger type, trigger bindings and channels. None of them calls an iii-stream function or binds an iii-stream trigger type, and they all run on an engine **without** iii-stream.

| Recipe | Existing requirement | Replaces | Replacement |
|---|---|---|---|
| [`owned-trigger-type.ts`](./owned-trigger-type.ts) | Ephemeral change/lifecycle notification | iii-stream `set` / `send` used only to notify; the `stream` trigger type or the Streams WebSocket to listen | The domain worker registers its own trigger type (`orders::changed`); consumers bind a function to it |
| [`stored-records.ts`](./stored-records.ts) | Queryable records with live updates | iii-stream `set` / `get` / `list` / `delete` as a generic KV store plus the `stream` trigger type to watch it | Explicit storage owned by the domain worker, `tasks::get` + paginated `tasks::list`, and a `tasks::changed` trigger fired after commit with `{ id, revision, op }`; consumers do an initial read, reconcile by revision and resync after reconnect |
| [`channel.ts`](./channel.ts) | Continuous content for one operation | Writing progress or partial output into stream items per job and watching them over the WebSocket | A channel created for the operation, with an explicit lifetime (deadline, cancel, close) and bounded buffering |

A trigger is a notification mechanism, not a database or a durable event log. If consumers need history or replay, keep and query a source of truth (recipe 2); do not rebuild an unbounded event store.

## Old to new, call by call

| Before (iii-stream) | After |
|---|---|
| iii-stream `set` to store a record | Your domain function writes to your storage (database, or `state::set` from the `state` worker), **then** fires its own trigger type |
| iii-stream `get` / `list` / `list_groups` | Your own `<worker>::get` and paginated `<worker>::list` functions |
| iii-stream `send` (ephemeral event) | `iii.trigger({ function_id: binding.function_id, ..., action: TriggerAction.Void() })` to each matching binding of your trigger type. iii-stream `send` already skips the KV write but still depends on iii-stream and has a different event envelope, so switching `set` to `send` is not a migration |
| A binding of the `stream` trigger type with `{ stream_name, group_id }` | `registerTrigger({ type: '<worker>::changed', config: { ...your validated filter } })` |
| Groups, `stream:join` / `stream:leave`, the Streams `auth_function` | A required tenant/scope key in the trigger config that the provider validates and filters on; who may bind is enforced with worker-manager RBAC (`on_trigger_registration_function_id`). A transport change alone does not reproduce these policies |
| Browser or Streams WebSocket subscription | The Browser SDK binds the same trigger types (it is a worker too); long content goes over a channel |

## What each recipe demonstrates

**`owned-trigger-type.ts`** (provider `orders`, consumer `billing`)
- The provider owns `registerTrigger` / `unregisterTrigger`, keeps its own binding registry and caps it (`MAX_BINDINGS`).
- Filters are validated at registration: unknown keys, a missing `tenant_id` or unknown event names reject the binding, and the binder's SDK logs `Trigger registration failed`.
- Delivery forwards the binding's `namespace` and `metadata` (config `metadata` first, then binding metadata) and uses `TriggerAction.Void()`, one send per matching binding. The payload is small and contains ids only.
- Tenant isolation: a `tenant-2` event never reaches a `tenant-1` binding.
- Cleanup: `binding.unregister()` (or the consumer disconnecting) reaches the provider and delivery stops.

**`stored-records.ts`** (provider `tasks`, consumer `dashboard`)
- Storage is a clearly labelled **in-memory demo store**, so the recipe runs on a bare engine. Swap in a database or the `state` worker; keep the contract that writes are committed before the notification and every write bumps a per-record revision (with tombstones for deletes).
- The consumer binds first, then reads (paginated, `PAGE_SIZE = 2` to show pages).
- Notify, then query: each notification is `{ id, revision, op }`; the consumer re-reads unless it already has that revision, so duplicates and out-of-order deliveries are skipped. A single drain loop with a bounded dirty set (`MAX_DIRTY`) avoids one task per event, and overflow falls back to one resync.
- Recovery: after a reconnect (`getConnectionState()` back to `connected`) or a restart with a cached view, the consumer re-queries everything and settles deletions through the tombstone revision. A failed read is retried as one resync with capped backoff.

**`channel.ts`** (producer `reports`, caller)
- The caller creates the channel (`createChannel(iii, 16)`, bounded engine buffer) and passes `writerRef` in the call. The producer writes NDJSON lines and awaits `'drain'`, so its local buffer stays at the Writable high-water mark. The reader's `Readable` pauses the socket when full, and one line is capped at 64 KiB.
- Lifetime: the producer always ends the writer (done, deadline or error). The caller has its own deadline and can stop early with `reports::cancel` plus closing its end. The producer caps concurrent operations and yields to the event loop so it can see a cancel.

## Run them

Prerequisites: Node.js 23.6+ (it runs `.ts` files natively) or Bun, the workspace installed (`pnpm install` at the repository root) and the SDK built (`pnpm --filter iii-sdk build`).

1. Start an engine **without iii-stream**. A minimal engine config is enough:

   ```yaml
   # /tmp/streams-migration/config.yaml
   workers:
     - name: iii-worker-manager
       config:
         port: 49134
   ```

   ```sh
   cd /tmp/streams-migration && iii --config config.yaml
   ```

   (The `worker-compose.yaml` in this package still declares iii-stream for the legacy `src/stream.ts` example; the recipes do not need it.)

2. From `sdk/packages/node/iii-example`, run any recipe. Each one starts its provider and consumer workers, checks the behaviour, prints `[demo] <recipe>: PASS` and exits `0` (or prints `FAIL` and exits `1`):

   ```sh
   III_URL=ws://127.0.0.1:49134 node --no-warnings src/streams-migration/owned-trigger-type.ts
   III_URL=ws://127.0.0.1:49134 node --no-warnings src/streams-migration/stored-records.ts
   III_URL=ws://127.0.0.1:49134 node --no-warnings src/streams-migration/channel.ts
   ```

   `--no-warnings` only hides Node's `MODULE_TYPELESS_PACKAGE_JSON` notice for this package. `bun src/streams-migration/<file>.ts` also works. Under Bun, the `ws` shim does not implement `pause()` / `resume()`, so reader-side socket backpressure in `channel.ts` does not apply there.

3. Type-check the recipes:

   ```sh
   pnpm exec tsc -p src/streams-migration
   ```

## Provider and consumer checklist

Provider (the worker that owns the data or the events):
- Owns registration and unregistration of its trigger type, validates every filter and rejects bad ones.
- Forwards each binding's `namespace` and `metadata` on delivery; never merges metadata into the payload.
- Bounds fan-out (binding cap, fire-and-forget delivery, no per-event task, no queue for slow consumers).
- Emits only after the write is committed; payloads stay small (ids plus revision), and the data is read through queries.

Consumer:
- Binds, then does the initial read; tolerates duplicates and reordering (revision or cursor); re-queries after reconnect; unregisters on shutdown.
- Treats a lost notification as normal: the store, not the trigger, is the source of truth.
