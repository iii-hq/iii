---
type: how-to
function_id: sandbox::stop
title: Stop and tear down a running sandbox
---

# When to use

Call `sandbox::stop` to signal a sandbox to shut down and reap its
resources. The call is idempotent against already-stopped state — a
second stop on the same id returns success with no work done — but
**not** against never-existed state: an unknown `sandbox_id` (or one
that's already been reaped from the registry) returns **S002**, not a
silent success.

Reach for it when:

- An [`sandbox::exec`](iii://sandbox/exec) sequence is finished and you
  want to free the VM's CPU/RAM/disk.
- A crash-recovery routine wants to reap a stranded sandbox surfaced by
  [`sandbox::list`](iii://sandbox/list).
- An idle sandbox approaches its `idle_timeout_secs` and you'd rather
  end it explicitly than wait for the daemon's auto-stop.

# Inputs

```json
{
  "sandbox_id":  "550e8400-e29b-41d4-a716-446655440000",  // required UUID; same id sandbox::create returned
  "wait":        false                                      // optional; true blocks until the VM has fully exited
}
```

`sandbox_id` is required and must be a valid UUID (malformed -> **S001**).
`wait` defaults to `false` (fire-and-forget within the daemon's grace
window); set `true` when downstream code needs the rootfs unmounted
before continuing.

# Outputs

```json
{
  "sandbox_id":  "550e8400-e29b-41d4-a716-446655440000",  // echoes the input id
  "stopped":     true                                       // true once the stop signal took effect within the grace window
}
```

- `stopped: true` means the daemon confirmed the VM exited (or was
  already stopped).
- `stopped: false` means the stop didn't complete in time — the sandbox
  may still be tearing down; poll [`sandbox::list`](iii://sandbox/list)
  after a few seconds to confirm.
- Once reaped, the row disappears from `sandbox::list`; a subsequent
  `sandbox::stop` against the same id then returns **S002**.

# Side effects

- Signals the underlying microVM to exit and frees its CPU / RAM / disk
  reservations.
- Removes the sandbox from the daemon's in-process registry once the
  shutdown completes (reaped rows are no longer returned by
  `sandbox::list`).
- Any in-flight `sandbox::exec` is interrupted; partial stdout/stderr
  may be observed in the exec response.

# Worked example

Fire-and-forget stop when you don't care about the exact teardown
moment:

```json
{ "sandbox_id": "550e8400-e29b-41d4-a716-446655440000", "wait": false }
```

Block-until-gone variant when downstream code needs the rootfs released:

```json
{ "sandbox_id": "550e8400-e29b-41d4-a716-446655440000", "wait": true }
```

# Related

- `sandbox::list` — confirm the sandbox is gone after a fire-and-forget stop.
- `sandbox::create` — boot a replacement once the previous one is reaped.
- `sandbox::exec` — drain pending work before issuing the stop.

## Errors

- **S001** `sandbox_id` is not a valid UUID.
- **S002** sandbox not found (unknown id, or already stopped and reaped — reaping removes the registry entry).
