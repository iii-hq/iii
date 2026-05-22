---
type: how-to
function_id: stream::update
title: Atomically apply ops to an item without read-modify-write
---

# When to use

Call `stream::update` when you want to change a stored item without
reading it first and risking a lost-write race against a concurrent
writer. Pass an ordered list of operations (`set`, `merge`, `increment`,
`decrement`, `append`, `remove`); the worker applies them atomically
against the current value and broadcasts the result the same way
`stream::set` does.

Reach for it when:

- Two writers can hit the same `(stream, group, item)` at once. `update`
  serializes the ops against the live value; `set` would let the second
  writer clobber the first.
- You want to mutate one field of an object without re-sending the rest
  (`merge` at root, or `set` at a `path`).
- You're maintaining counters (`increment`/`decrement`) or growing
  arrays/strings (`append`) where the pre-state is the source of truth.

Use [`stream::set`](iii://iii-stream/stream/set) instead when you have the full
desired value and concurrent writers aren't a concern — `set` is one
adapter round-trip; `update` does a read-then-write.

Use [`stream::delete`](iii://iii-stream/stream/delete) instead when the goal is
removing the item entirely. `remove` ops only delete fields inside the
current value.

# Inputs

```json
{
  "stream_name": "counters",                   // required; top-level namespace
  "group_id":    "global",                     // required; second-level partition
  "item_id":     "page-views",                 // required; identifier within the group
  "ops": [                                     // required; ordered list, applied left-to-right
    { "type": "increment", "path": "total",        "by": 1 },
    { "type": "set",       "path": "last_seen_at", "value": "2026-05-20T17:00:00Z" },
    { "type": "merge",                              "value": { "source": "web" } }
  ]
}
```

All four fields are required. `ops` accepts six shapes; note that
**only `merge` and `append`** accept a nested-path or omitted-path
form. `set`/`increment`/`decrement`/`remove` use `FieldPath`, which is
a single string (empty string means root for `set`).

- `{ "type": "set", "path": "<field>", "value": <any | null> }` —
  overwrite a field, or replace the root value when `path` is `""`.
  `path` is a `FieldPath` (single string). `value: null` writes a JSON
  null; omit the field entirely is a deserialization error.
- `{ "type": "merge", "path": <"field" | ["a","b","c"] | omitted>, "value": <object> }` —
  shallow-merge an object at root, at a first-level key, or at a
  nested path of literal segments. **Object-only** — the `value` must
  be a JSON object. `path` is a `MergePath`:
  - omitted, `""`, or `[]` → root merge.
  - `"foo"` is equivalent to `["foo"]` (single first-level key).
  - `["a", "b", "c"]` walks three literal keys; dots inside a segment
    are treated as part of the literal name (`["a.b"]` is one key
    named `"a.b"`, not nested `a → b`).
- `{ "type": "increment", "path": "<field>", "by": <i64> }` —
  add to a numeric field. `path` is a `FieldPath` (single string;
  no nested paths).
- `{ "type": "decrement", "path": "<field>", "by": <i64> }` —
  subtract from a numeric field. Same `FieldPath` shape as `increment`.
- `{ "type": "append", "path": <"field" | ["a","b","c"] | omitted>, "value": <any> }` —
  push one element to an array, or concatenate to a string. `path`
  follows the same `MergePath` rules as `merge`. Omit `path` (or send
  `""`) to append at root.
- `{ "type": "remove", "path": "<field>" }` — delete a field from the
  current object. `path` is a `FieldPath` (single string).

When the item doesn't exist, the worker treats the pre-state as `null`
and applies ops against it — most ops error in that case, but `set` at
root or `merge` at root will create the item.

# Outputs

```json
{
  "old_value": { "total": 41, "source": "web" },              // value before; null if the item didn't exist
  "new_value": { "total": 42, "last_seen_at": "2026-05-20T17:00:00Z", "source": "web" },
  "errors": [                                                 // omitted when empty; per-op failures from the ops array
    { "op_index": 2, "code": "merge.path.too_deep", "message": "merge depth exceeds max", "doc_url": null }
  ]
}
```

- `old_value` is `null` exactly when the item did not exist before the
  call.
- `new_value` reflects every op that **succeeded**. Failing ops are
  recorded in `errors` and skipped — the call is not aborted by a single
  bad op. Inspect `errors` after every call that mixes op types.
- `errors` is **omitted from the JSON envelope when empty** (it is not
  serialized as `[]`). Treat the absence of the field as "no failures".
- Each `UpdateOpError` carries `op_index` (zero-based position in the
  request `ops` array), a stable `code` (e.g. `merge.path.too_deep`),
  a human-readable `message`, and an optional `doc_url`.

# Side effects

Same shape as [`stream::set`](iii://iii-stream/stream/set):

1. **Synchronous, before the call returns** — the adapter persists
   `new_value` at `(stream_name, group_id, item_id)`.
2. **Asynchronous, fire-and-forget** — the worker spawns a task that
   invokes every registered [`stream`](iii://iii-stream/stream/reactive-triggers)
   trigger whose filter matches. Handlers run on that spawned task
   **after** the `update` call returns. Slow or failing handlers do
   not delay or fail the originating `update`.
3. **Synchronous, before the call returns** — a `StreamWrapperMessage`
   with `event.type: "create"` (when `old_value` was `null`) or
   `"update"` (otherwise) is broadcast to every WebSocket subscribed
   to the stream and group. `event.data` carries `new_value`.

The actual call-site sequence is **persist → spawn trigger task →
broadcast → return**, identical to `set`.

A call where every op failed but the underlying value was unchanged
still broadcasts and fires triggers — the broadcast carries the
unchanged `new_value`. Use `errors` to detect this case server-side.

# Worked example

Increment a page-view counter and stamp the last-seen timestamp in one atomic call:

```json
{
  "stream_name": "counters",
  "group_id":    "global",
  "item_id":     "page-views",
  "ops": [
    { "type": "increment", "path": "total",        "by": 1 },
    { "type": "set",       "path": "last_seen_at", "value": "2026-05-20T17:00:00Z" }
  ]
}
```

The response carries `old_value`, the post-write `new_value`, and (only when at least one op failed) an `errors` array with per-op `op_index` / `code` / `message`. `errors` is omitted entirely when every op succeeded, so the absence of the field is the success signal — see **Outputs** for the precise shape.

# Related

- `stream::set` — overwrite blindly when concurrent writers aren't a concern; one round-trip.
- `stream::get` — read the current value when you genuinely need to inspect it before deciding which ops to send.
- `stream::delete` — remove the entire item; `remove` ops only delete fields, not the item itself.
- `stream::send` — broadcast a transient event without modifying the item.
- `stream` trigger (`iii://iii-stream/stream/reactive-triggers`) — register a handler that fires on every successful update whose filter matches.
