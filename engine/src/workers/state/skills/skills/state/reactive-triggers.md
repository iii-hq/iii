---
type: how-to
trigger_type: state
title: React to state changes with triggers
---

# When to use

Register a `state` trigger when a function should run automatically after a value in the store changes — without polling `state::get` or wiring custom WebSocket push. The engine evaluates every registered `state` trigger after a successful `state::set`, `state::update`, or `state::delete` and invokes matching handlers asynchronously.

Reach for it when:

- A write in one function should kick off side effects in another (audit logs, cache invalidation, notifications, projections).
- You want to watch a specific `scope`/`key` pair (e.g. `orders` / `status`) and react only when that slot changes.
- You need optional gating with `condition_function_id` so the handler runs only when a predicate on the event returns truthy.

State does not push updates to SDK clients. If you only need the new value inside the same function that wrote it, call `state::get` or use the `old_value` / `new_value` returned by the mutator instead of registering a trigger.

Prerequisite: the `iii-state` worker must be enabled in `config.yaml` (it is on by default). Handlers and triggers are registered from a connected worker via `iii.registerFunction` and `iii.registerTrigger` — not through `state::*` engine functions.

# Inputs

Registration is a two-step pattern: define the handler function, then bind it to the `state` trigger type.

## Handler function

Register any function id. The handler receives the event payload documented in Outputs (same shape the engine passes to `condition_function_id` when configured).

```json
// Returned from the handler — shape is up to your application.
{ "handled": true }
```

## Trigger registration

```json
{
  "type":        "state",                      // required. Must be exactly "state".
  "function_id": "orders::on-status-change",   // required. Handler to invoke when the trigger matches.
  "config": {
    "scope":                   "orders",       // optional. Only fire for changes in this scope. Omit to match every scope.
    "key":                     "status",       // optional. Only fire for this key within the scope. Omit to match every key in the (filtered) scope.
    "condition_function_id":   "conditions::is-shipped"  // optional. Engine calls this with the event; handler runs only when it returns true.
  }
}
```

`type` and `function_id` are required. All `config` fields are optional; tighter filters reduce how often the handler runs.

| Config field | When omitted | When set |
|--------------|--------------|----------|
| `scope` | Matches changes in any scope | Matches only events whose `scope` equals this string |
| `key` | Matches any key in the matched scope(s) | Matches only events whose `key` equals this string |
| `condition_function_id` | Handler runs whenever scope/key match | Engine invokes the named function with the event; handler is skipped on `false` or condition error |

To watch multiple specific keys, register one trigger per `scope`/`key` pair (or omit `key` to receive every change in a scope and filter inside the handler).

Mutations that **do** fire triggers: `state::set`, `state::update`, `state::delete`. Reads (`state::get`, `state::list`, `state::list_groups`) do not.

# Outputs

When a trigger matches, the engine invokes `function_id` with this event object:

```json
{
  "type":       "state",                       // Always "state".
  "event_type": "state:updated",               // "state:created" | "state:updated" | "state:deleted".
  "scope":      "orders",
  "key":        "status",
  "old_value":  { "orderId": "order-123", "status": "pending" },  // null when the key was newly created.
  "new_value":  { "orderId": "order-123", "status": "shipped" }   // null when the key was deleted.
}
```

- `event_type` is `state:created` when the key did not exist before the write (`old_value` was null on set/update), `state:updated` when it did, and `state:deleted` on delete (`new_value` is always null).
- A delete of an already-missing key still emits `state:deleted` with `old_value: null`; handlers that should ignore no-ops must check `old_value`.
- Trigger delivery is asynchronous: the `state::set` / `state::update` / `state::delete` caller returns before handlers finish. Handler failures do not roll back the write.
- Every matching trigger receives a copy of the same event payload; multiple triggers on the same `scope`/`key` each run independently.

# Worked example

Watch `orders` / `status` and react when another worker updates it.

Register the handler:

```json
// iii.registerFunction — handler id only; no engine payload.
{ "id": "orders::on-status-change" }
```

Register the trigger:

```json
{
  "type":        "state",
  "function_id": "orders::on-status-change",
  "config":      { "scope": "orders", "key": "status" }
}
```

From any connected worker, write state to fire the handler:

```json
{
  "function_id": "state::set",
  "payload": {
    "scope": "orders",
    "key":   "status",
    "value": { "orderId": "order-123", "status": "shipped" }
  }
}
```

The handler receives `event_type: "state:updated"` (or `"state:created"` on first write) with `new_value` set to the shipped payload.

# Related

- `state::set` — simplest write path that fires create/update events.
- `state::update` — partial mutations that also fire create/update events.
- `state::delete` — removes a key and fires `state:deleted`.
- `state::get` — read the current value without registering a trigger.
