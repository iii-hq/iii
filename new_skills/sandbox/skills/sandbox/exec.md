---
type: how-to
function_id: sandbox::exec
title: Run a command inside a running sandbox
---

# When to use

Call `sandbox::exec` to execute a command inside a sandbox you previously
booted with [`sandbox::create`](iii://sandbox/create), streaming stdout
and stderr back to the caller. Each call is serialized per `sandbox_id`:
only one exec runs at a time on a given sandbox, and a concurrent call
against the same id returns **S003** while the previous one is still in
flight. The call returns when the child exits, the caller-set timeout
fires, or the VM becomes unreachable.

Reach for it when:

- You have a `sandbox_id` from `sandbox::create` and want to run a build,
  test, or script step inside it.
- You're stepping through a multi-command workflow and need each step to
  complete (or fail) before the next one starts.
- You want POSIX-style exit-code semantics over `success`/`exit_code`
  fields rather than an error envelope.

Use [`sandbox::create`](iii://sandbox/create) first when you don't yet
have a `sandbox_id`. To run two commands in parallel, boot two sandboxes.

# Inputs

```json
{
  "sandbox_id":  "550e8400-e29b-41d4-a716-446655440000",  // required UUID from sandbox::create
  "cmd":         "bash",                                   // required; the binary as a single string, NOT an argv array
  "args":        ["-lc", "echo hello && date"],            // optional argv tail (Vec<String>)
  "stdin":       "aGVsbG8K",                               // optional base64-encoded bytes piped to the child's stdin
  "env":         ["FOO=bar"],                              // optional Vec<String> of "K=V" entries (NOT a map)
  "workdir":     "/workspace",                             // optional cwd inside the sandbox; image default applies when omitted
  "timeout_ms":  30000                                     // optional kill-after window in ms; daemon default applies when omitted
}
```

`sandbox_id` and `cmd` are required. `stdin` must be valid base64;
malformed base64 surfaces as **S001**. An unknown `sandbox_id` returns
**S002**.

# Outputs

```json
{
  "stdout":       "hello\nMon Oct 13 19:42:11 UTC 2025\n",  // captured stdout up to the close of the call
  "stderr":       "",                                        // captured stderr up to the close of the call
  "exit_code":    0,                                         // POSIX exit code; null/omitted only when the child never started
  "timed_out":    false,                                     // true iff timeout_ms fired before the child exited
  "duration_ms":  42,                                        // wall-clock time spent in the call
  "success":      true                                       // exit_code == 0 AND timed_out == false
}
```

- `success` is `true` iff `exit_code == 0` *and* `timed_out == false`;
  callers that only check `success` get correct fail-on-timeout behaviour
  for free.
- Per POSIX, spawn failures surface in `exit_code`, NOT as an error
  envelope: `127` means "command not found"; `126` means "not executable".
- `stdout` / `stderr` captured before a timeout are still returned
  alongside `timed_out: true`, so callers can inspect what ran.

# Worked example

Run a short shell pipeline with a 30s ceiling:

```json
{
  "sandbox_id": "550e8400-e29b-41d4-a716-446655440000",
  "cmd":        "bash",
  "args":       ["-lc", "echo hello && date"],
  "env":        ["FOO=bar"],
  "workdir":    "/workspace",
  "timeout_ms": 30000
}
```

Returns `{ "stdout": "hello\n…\n", "exit_code": 0, "success": true, ... }`.
A failing command (e.g. `"cmd": "nope"`) returns `exit_code: 127,
success: false` — still a normal response, not an error envelope.

# Related

- `sandbox::create` — boot the sandbox before you can exec inside it.
- `sandbox::list` — check `exec_in_progress` before retrying after **S003**.
- `sandbox::stop` — tear the sandbox down once the exec sequence is done.

## Errors

- **S001** invalid request (bad `sandbox_id` UUID, missing `cmd`).
- **S002** sandbox not found.
- **S003** concurrent exec — await the previous one first.
- **S200** exec timed out (stdout/stderr captured pre-timeout are still returned).
- **S300** VM unreachable (boot failed earlier, or shell socket dropped).
