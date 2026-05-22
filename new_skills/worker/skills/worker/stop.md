---
type: how-to
function_id: worker::stop
title: Gracefully stop a running worker
---

# When to use

Call `worker::stop` to send a graceful shutdown signal to a running
worker. It's destructive — losing in-flight requests is possible — so
the trigger surface requires explicit `yes: true` consent (**W104**
otherwise). The op is stateful: already-stopped workers return success
with `stopped: true`, while unknown names return **W110**.

Reach for it when:

- A worker is in a bad state and you want to drain it before
  [`worker::start`](iii://worker/start) brings it back.
- You're decommissioning a worker and want to halt the process before
  calling [`worker::remove`](iii://worker/remove).
- A test harness needs each scenario to start from a known
  not-running state.

Use [`worker::remove`](iii://worker/remove) instead when you also want
to drop the entry from `iii.config.yaml`. Use
[`sandbox::stop`](iii://sandbox/stop) when the thing you want to stop
is a sandbox VM, not a worker.

# Inputs

```json
{
  "name":  "image-resize",  // required; installed worker name
  "yes":   true             // required consent flag; must be exactly true
}
```

`name` is required and must match `[a-z0-9_-]{1,64}` (**W100** on
violation) and refer to an installed worker (**W110** otherwise).

`yes` must be exactly `true` — not `false`, not omitted, not the string
`"true"`, not the number `1`. A slip in caller code should not silently
kill a running worker; the trigger surface rejects with **W104** rather
than guess.

# Outputs

```json
{
  "name":     "image-resize",  // echoes the input name
  "stopped":  true              // true once the worker has exited within the daemon's grace window
}
```

- `stopped: true` means the worker confirmed shutdown (or was already
  stopped before the call).
- `stopped: false` means the stop didn't take effect within the
  daemon's grace window — retry, or verify with
  [`worker::list`](iii://worker/list).
- A `stopped: true` response does **not** remove the entry from
  `iii.config.yaml`; the worker can be brought back up with
  [`worker::start`](iii://worker/start) without re-installing.

# Side effects

- Sends a graceful shutdown signal to the worker process (binary or
  OCI). The process gets the daemon's grace window to exit cleanly
  before being force-killed.
- Closes the engine's WebSocket connection to the worker.
- [`worker::list`](iii://worker/list) now reports `running: false` for
  the same `name`.
- Does **not** touch `iii.config.yaml`, `iii.lock`, or
  `~/.iii/managed/{name}/` — only process state changes.
- Publishes lifecycle events to the [`worker`](iii://worker/events)
  trigger type: `started` → `stopping` → `done` on success, or
  `started` → `stopping` → `failed` on error. Subscribers filter via
  `WorkerTriggerConfig`.

# Worked example

Graceful stop with explicit consent:

```json
{ "name": "image-resize", "yes": true }
```

Returns `{ "name": "image-resize", "stopped": true }` once the worker
has exited within the grace window.

# Related

- `worker::list` — confirm `running: false` after the call.
- `worker::start` — bring the worker back up.
- `worker::remove` — also drop the config entry (pairs with this op
  during decommission).
- [`worker`](iii://worker/events) — subscribe to lifecycle events fired
  by this op.

## Errors

- **W100** invalid worker name (shell metacharacters, empty, > 64 chars).
- **W104** `yes` not `true`.
- **W110** worker name unknown.
- **W900** signal failure or grace-window timeout.
