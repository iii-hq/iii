---
type: how-to
function_id: worker::list
title: List installed workers and their run state
---

# When to use

Call `worker::list` to read the union of every worker in
`iii.config.yaml`, every artifact on disk under `~/.iii/managed/`, and
every running process the daemon can see. It's a pure read — safe to
poll, idempotent — so reach for it before targeting any other op when
you don't already know what's installed.

Reach for it when:

- You're verifying that [`worker::add`](iii://worker/add) /
  [`worker::remove`](iii://worker/remove) did what you expected.
- A bootstrap script is enumerating installed workers and starting any
  that show `running: false`.
- You want to inspect pinned versions before calling
  [`worker::update`](iii://worker/update).

# Inputs

```json
{
  "running_only":  false  // optional; true filters the response to workers with `running: true`
}
```

No required fields. `running_only: true` filters out idle workers — use
it when you only care about the live process roster.

# Outputs

```json
{
  "workers": [
    { "name": "iii-stream",   "running": true, "pid": null },
    { "name": "image-resize", "running": true, "pid": 37037, "version": "0.1.2" },
    { "name": "skills",       "running": true, "pid": 37036, "version": "0.2.4" }
  ]
}
```

Each `WorkerEntry`:

- `name` — config name as it appears in `iii.config.yaml`.
- `version` — semver string for registry-tracked workers; **omitted from
  the JSON entirely** for engine builtins that aren't lock-tracked
  (callers should treat absence as "no pinned version" rather than
  empty string).
- `running` — `true` when the daemon currently has a WebSocket
  connection to the worker.
- `pid` — OS pid discovered via `ps`; `null` for engine builtins that
  don't surface a process (e.g. `iii-stream`, `iii-http`).
- Rows are sorted lexicographically by `name`.

# Worked example

List everything regardless of run state:

```json
{ "running_only": false }
```

Or only the currently-running roster:

```json
{ "running_only": true }
```

# Related

- `worker::add` — install a worker first so it shows up here.
- `worker::start` — kick off any rows with `running: false` that should
  be live.
- `worker::schema` — discover the exact field shapes (default timeouts,
  idempotency hints) for each op.

## Errors

- **W900** filesystem read failure (rare — config/lock both unreadable).
