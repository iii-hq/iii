---
type: how-to
function_id: worker::start
title: Spawn a configured worker and wait for the ready signal
---

# When to use

Call `worker::start` to spawn a worker that's already present in
`iii.config.yaml`. The engine connects to the worker over its WebSocket
port and (by default) blocks until the worker reports ready. The op is
stateful: starting an already-running worker is a silent no-op (the same
`pid` / `port` come back), while starting an unknown worker returns
**W110**.

Reach for it when:

- You just ran [`worker::add`](iii://worker/add) and need the worker
  process live before the engine can route calls to it.
- A worker crashed and you want to bring it back up after diagnosing.
- A scripted bootstrap is enumerating
  [`worker::list`](iii://worker/list) rows and starting any that show
  `running: false`.

Use [`worker::add`](iii://worker/add) instead when the worker isn't yet
installed — start only handles workers that already have an
`iii.config.yaml` entry.

# Inputs

```json
{
  "name":    "image-resize",                 // required; must match an installed worker
  "port":    49134,                          // optional override of the engine WS port; defaults to `iii-worker-manager`'s port
  "config":  "./image-resize.config.yaml",   // optional; forwarded as `--config <path>` to binary workers (OCI workers ignore this)
  "wait":    true                            // optional; true blocks until the worker reports ready (default)
}
```

`name` is required and must match `[a-z0-9_-]{1,64}` (otherwise **W100**)
and refer to an installed worker (otherwise **W110**). `config` only
applies to binary workers; OCI workers silently ignore it.

# Outputs

```json
{
  "name":  "image-resize",  // echoes the input name (resolved to the canonical case in iii.config.yaml)
  "pid":   12345,           // OS pid of the spawned process; null for engine builtins (e.g. iii-stream, iii-http)
  "port":  49134            // resolved WS port the engine is connected on; null when the builtin doesn't surface one
}
```

- `pid` is `null` for engine builtins that don't surface a process
  (e.g. `iii-stream`, `iii-http`); for binary and OCI workers it's the
  OS pid you can pass to `kill`.
- `port` is the resolved WS port the engine is connected on (either the
  passed override or the default `iii-worker-manager` port); `null` for
  builtins that don't surface a port.
- An already-running worker returns the existing `pid` / `port` rather
  than respawning.

# Side effects

- Spawns the worker process (binary, OCI container, or builtin
  goroutine) and registers it with the engine's worker manager.
- Opens a WebSocket connection from the engine to the worker; the
  process now appears as `running: true` in
  [`worker::list`](iii://worker/list).
- When `wait: true` (the default), the call blocks until the worker
  signals ready or the daemon's grace window expires.

# Worked example

Start an installed worker and wait for it to come up:

```json
{ "name": "image-resize", "wait": true }
```

Returns `{ "name": "image-resize", "pid": 12345, "port": 49134 }`. A
second call with the same input returns the same `pid` and `port`
(no-op).

# Related

- `worker::list` — confirm `running: true` after the call.
- `worker::stop` — graceful shutdown (consent required).
- `worker::add` — install the worker before you can start it.

## Errors

- **W100** invalid name.
- **W110** worker not installed.
- **W900** spawn failure, ready-wait timeout, or port-bind failure.
