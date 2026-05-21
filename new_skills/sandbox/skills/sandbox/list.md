---
type: how-to
function_id: sandbox::list
title: List running and recently-stopped sandboxes
---

# When to use

Call `sandbox::list` to read the current sandbox roster the daemon knows
about. It's a pure read — safe to poll, idempotent, and never mutates
state — so reach for it before targeting any other op when you don't
already have a `sandbox_id` in hand.

Reach for it when:

- You're recovering from a crash and need to discover what's still alive.
- You hit **S003** on [`sandbox::exec`](iii://sandbox/exec) and want to
  confirm whether the previous exec is still in flight (`exec_in_progress`).
- You're about to call [`sandbox::stop`](iii://sandbox/stop) and want to
  filter to running sandboxes (`stopped == false`).

# Inputs

```json
{}
```

No parameters. Empty body is the only valid input.

# Outputs

```json
{
  "sandboxes": [
    {
      "sandbox_id":        "550e8400-e29b-41d4-a716-446655440000",  // UUID; pass to exec / stop
      "name":              null,                                     // optional human label set at create time; null when unset
      "image":             "ghcr.io/iii-hq/node:latest",             // resolved OCI ref the sandbox booted from
      "age_secs":          142,                                      // seconds since sandbox::create returned
      "exec_in_progress":  false,                                    // true while a sandbox::exec call is running on this id
      "stopped":           false                                     // false for live sandboxes
    }
  ]
}
```

- `name` is `null` when the create call omitted the label; otherwise it
  echoes the exact string the caller passed.
- There is no `running` field — derive it from `!stopped`.
- A row with `exec_in_progress: true` will reject a parallel
  `sandbox::exec` call with **S003** until the running exec settles.
- The roster includes recently-stopped sandboxes until the daemon reaps
  them; after reaping, a `sandbox::stop` against the same id returns
  **S002**.
- Always returns the current roster (possibly empty). An unexpected
  registry read failure surfaces as **S300**.

# Worked example

Poll the roster after a `sandbox::create`:

```json
{}
```

Returns one entry per live (or just-stopped) sandbox with the same
`sandbox_id` the create call returned, the resolved `image`, and zero
`age_secs` immediately after boot.

# Related

- `sandbox::create` — discover the id you'll see in this list.
- `sandbox::exec` — target a specific row by `sandbox_id`.
- `sandbox::stop` — pick a row with `stopped: false` to tear down.

## Errors

`sandbox::list` doesn't surface errors in practice — it always returns
the current roster (possibly empty). An unexpected registry read failure
surfaces as **S300** (platform).
