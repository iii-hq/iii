---
title: iii call for VM workers — sandbox::build, sandbox::start and a streaming iii call
tagline: "A VM container's scripts become iii call lines: scripts.build is iii call sandbox::build worker=<container> and scripts.run is iii call sandbox::start worker=<container>. iii call follows the streams a call carries and turns Ctrl+C into a stop, so the iii sandbox CLI goes away."
date: 2026-11-19
tags: [compose, sandbox, cli, streams]
status: draft
---

# iii call for VM workers

KAN-293, items 1 and 2. Builds on the explicit VM mode
([2026-11-10-explicit-vm-mode](../2026-11-10-explicit-vm-mode/README.md)) and on the 0.27 SDK
streams (#2561).

## 1. Why

`iii sandbox build` and `iii sandbox run` exist because `iii call` could not stream: a one-shot
call returns at once, so Compose would have nothing to supervise (#2515, r26). Since #2561 a
stream is a value of a call, in its payload or its result. What is still missing is the CLI side:
`iii call` prints a stream's ref and exits, and a signal just kills it.

Goals:

- A VM container's scripts are `iii call` lines. The `iii sandbox` command is removed.
- `iii call` follows the streams a call carries, and a signal stops the call instead of killing it.
- One behavior inside and outside Compose (r909): an operator rebuilds with
  `iii call compose::build worker=<c>`, which follows the build the same way.
- `scripts.run` still decides host or VM, as KAN-293 item 6 wants.

Not here: one adapter without the `<adapter>::` prefix (item 3), `${self.*}` (item 4), the
single migration with a format marker (item 6), any adapter contract change.

## 2. The compose file

```yaml
onboarding:
  worker: package://api.workers.iii.dev/onboarding
  version: "0.2.0"
  start_after: [sandbox-libkrun]
  scripts:
    build: iii call sandbox::build worker=onboarding
    run: iii call sandbox::start worker=onboarding
```

- A `path://` worker that runs its own command in the guest adds it as a field:
  `iii call sandbox::start worker=link command='python main.py'`.
- A container runs in a VM when the first three words of its `scripts.run` are
  `iii call sandbox::start`; `scripts.build` must then start with `iii call sandbox::build`. The
  rules of the explicit VM mode (its section 4.2) apply unchanged to these forms.
- `worker=` must name the container itself. Compose checks it offline and refuses another name
  with `SANDBOX_WORKER_MISMATCH`, so a renamed container fails on load, not on a startup timeout.
  Once `${self.*}` lands (item 4), Compose writes `worker=${self.name}`.
- Everything else the call needs comes from Compose (4.2), so the line stays this short. Run by
  hand, outside Compose, the call takes every field as an argument:
  `iii call sandbox::start --help` lists them.
- The rewrite of 0.26 files writes these forms, and also rewrites the forms of the explicit VM mode
  (`iii sandbox build`, `iii sandbox run [--command '<c>']`) that the branch wrote before this spec.

## 3. `iii call`

This section is delivered as its own pull request into `0.27.0`, then merged into the explicit VM
mode branch.

### 3.1 Streams

- **Bound in the payload.** Before the call, `iii call` reads the function's request schema from
  `engine::functions::info`, as `--help` already does. An `OutputStream` field named `stdout` gets
  a sink that writes to this process's stdout, `stderr` to its stderr, and an `InputStream` field
  named `stdin` reads this process's stdin. `--stdout <field>`, `--stderr <field>` and
  `--stdin <field>` bind other fields; `--no-bind` binds none.
- **Followed in the result.** An `InputStream` at `/stdout` or `/stderr` of the result is copied
  to that descriptor until it ends. `--no-follow` prints the refs and exits, as today.
- **The result.** With no stream bound or followed, the result JSON goes to stdout, as today.
  With one, it is printed only with `--print-result`, after every stream ended, so it never mixes
  with the streamed output.
- **Errors.** As today, `Error: {code, message, stacktrace}` on stderr, on one line when stderr is
  not a terminal, so a caller such as Compose reads `code` and `message`. A followed stream that is
  aborted is an error too.

### 3.2 Timeout

With a bound or followed stream, the call has no timeout unless `--timeout-ms` sets one: the
function's own budget applies (`timeout_ms` of `sandbox::build`, for example). Without one, the
30 s default stays.

### 3.3 Interrupt as stop

- The first SIGINT or SIGTERM cancels every bound and followed stream. The function sees its sink
  close (a write fails, or the engine closes the writer when the reader leaves), stops what it
  runs and answers. `iii call` waits for that answer and exits with it.
- A second signal exits at once.
- With no stream, the first signal exits at once, as today.

This is the contract a long-running function relies on: it lasts as long as its caller reads.

### 3.4 Defaults from the environment

`III_CALL_DEFAULTS` holds a JSON object keyed by function id:

```json
{
  "sandbox::start": { "auth": "...", "state_key": "...", "source": { "kind": "bundle", "...": "..." } },
  "sandbox::build": { "auth": "...", "state_key": "...", "timeout_ms": 1200000 }
}
```

A call to `F` takes `III_CALL_DEFAULTS[F]` as its base payload; `--json` and `key=value` pairs
override its keys. Only the function the entry names receives it: an `iii call state::get` in the
same script sees nothing. The values never appear in the process's arguments, so `ps` shows no
token and no secret from the worker's environment. A variable that is not a JSON object, or an
entry that is not one, is a usage error. `iii call --help` documents the variable.

### 3.5 Exit codes

| Code  | When                                                                  |
| ----- | --------------------------------------------------------------------- |
| `0`   | The call succeeded and every followed stream ended.                   |
| `1`   | The function failed, or a followed stream was aborted.                |
| `2`   | Usage error.                                                          |
| `130` | Interrupted by SIGINT with no answer (a second signal, or no stream). |
| `143` | The same for SIGTERM.                                                 |

### 3.6 New flags and variables

| Flag or variable                          | Default                                              |
| ----------------------------------------- | ---------------------------------------------------- |
| `--stdout`, `--stderr`, `--stdin <field>` | `stdout`, `stderr`, `stdin` when the schema has them |
| `--no-bind`                               | off                                                  |
| `--no-follow`                             | off                                                  |
| `--print-result`                          | off when a stream is bound or followed               |
| `III_CALL_DEFAULTS`                       | unset                                                |

## 4. `sandbox::build` and `sandbox::start`

### 4.1 Who serves them

The engine's `iii-sandbox` router publishes them in `default`, beside the ephemeral `sandbox::*`
operations. They compose the adapter's contract 1.4 calls (`adapter::info`, `worker::build`,
`worker::start`, `worker::logs`, `worker::status`, `worker::stop`): the code of
`iii sandbox build` and `iii sandbox run` moves from `crates/iii-compose/src/sandbox_cli.rs` into
the router. `sandbox-libkrun` `^0.5` works unchanged, so no adapter release is needed.

Unlike the ephemeral operations, they go to the adapter the request names (`adapter`,
`namespace`), not to the router's binding: a VM worker uses its own project's adapter, which may
run in the project's namespace. Item 3 of KAN-293 later moves them into the adapter itself.

### 4.2 Request

Compose writes the request of each VM container into the `sandbox::build` and `sandbox::start`
entries of `III_CALL_DEFAULTS`, in the environment of that container's scripts. The variable
replaces `III_SANDBOX_TOKEN`, `III_SANDBOX_STATE_KEY`, `III_SANDBOX_ADAPTER`,
`III_SANDBOX_NAMESPACE`, `III_WORKER_ENV_KEYS`, the `III_BUILD_*` variables and
`III_PACKAGE_KIND`. It never reaches the guest and is never listed in the worker's environment.

| Field              | Used by | Value                                                                  |
| ------------------ | ------- | ---------------------------------------------------------------------- |
| `worker`           | both    | The container key, written on the script's line.                       |
| `adapter`          | both    | The adapter's name, such as `sandbox-libkrun`.                         |
| `namespace`        | both    | Where its functions are registered.                                    |
| `auth`             | both    | The VM's scoped token.                                                 |
| `state_key`        | both    | The VM's state key.                                                    |
| `source`           | both    | `WorkerSource`: bundle, binary or local, with its host path.           |
| `env`              | both    | The worker's own variables, the guest's whole environment.             |
| `build_dir`        | both    | `<project>/.iii/build/<container>`.                                    |
| `base_image`       | build   | The container's `runtime.base_image`, when set.                        |
| `timeout_ms`       | build   | `scripts.build_timeout`.                                               |
| `build_id`         | start   | From `build.json`, which Compose reads before it starts the container. |
| `engine_url`       | start   | What the guest connects to.                                            |
| `stop_grace_ms`    | start   | Compose's stop timeout, minus a margin for the answer.                 |
| `command`          | start   | The guest command of a `path://` worker; refused on a package.         |
| `stdout`, `stderr` | both    | `OutputStream` sinks, bound by `iii call`.                             |

### 4.3 `sandbox::build`

Checks `adapter::info` (`worker.build`, and `worker.binary` for a binary or a `base_image`), then
calls `worker::build` with a logs channel and writes each record's line to `stdout` or `stderr`.
Answers `{ build_id, base_image: { reference, digest }, built_at_ms, adapter, contract }`.

- Its sink closing closes the logs channel, so the adapter aborts the build and keeps the
  previous image: `OPERATION_CANCELLED`.
- Past `timeout_ms`: the same abort, `TIMED_OUT`.
- An adapter failure keeps its S-code as the error code.
- It writes no file: the engine may run on another machine. Compose records `build.json` (5.2).

### 4.4 `sandbox::start`

Boots `build_id` from `build_dir` with `worker::start` and `build`, or reattaches to the live boot
of the same state (S502), and copies the guest's lines to `stdout` and `stderr`. It writes an
empty frame to them every few seconds, so a reader that left is noticed with a silent guest too.

- Its sinks closing (Compose's SIGTERM, a Ctrl+C) stops the VM with `worker::stop` and
  `stop_grace_ms`, then answers `{ stopped: true, exit_code }`. A caller that dies (SIGKILL, a
  lost connection) closes them too, so its VM is stopped the same way: no VM outlives its call.
- A boot it reattached to that ends as stopped was being stopped by an earlier start whose caller
  died. That end is not this call's, so it boots once more in the same call and writes
  `sandbox::start: VM <vm_id> instance <n> was stopped by an earlier start; booting it again` to
  `stderr`. A second such end is answered as an exit, below.
- The VM exiting with 0 answers `{ exit_code: 0 }`. Exiting otherwise fails with
  `WORKER_EXITED`, its code or signal in the message, so `iii call` exits 1. Compose's restart
  policy only tells zero from non-zero, so it is unaffected; the VM's own code is in the error, in
  the container's logs.
- A `build_id` that does not match `image.id` fails with S508, as today; the message asks for
  `iii call compose::build worker=<container>`.

### 4.5 Errors

`SANDBOX_BUILD_UNSUPPORTED`, `SANDBOX_BINARY_UNSUPPORTED`, `SANDBOX_CONTRACT_UNSUPPORTED`,
`TIMED_OUT`, `OPERATION_CANCELLED` and the adapter's S-codes keep their meaning.
`SANDBOX_INVALID_REQUEST` replaces `SANDBOX_ENV_INVALID` and `SANDBOX_INVALID_ARGUMENTS`.
`WORKER_EXITED` is new. `BUILD_MISSING` moves to Compose, which refuses the start before it
spawns the script. `SANDBOX_PRUNE_UNSUPPORTED` stays with `compose::remove`.

> **As built.** `BUILD_MISSING` is not raised. Compose builds a container before it starts it, so a
> start always finds `build.json`, and Compose leaves the `sandbox::start` entry out of
> `III_CALL_DEFAULTS` until it does. A hand-run `sandbox::start` without `build_id` fails with
> `SANDBOX_INVALID_REQUEST`. `compose::remove` does not raise `SANDBOX_PRUNE_UNSUPPORTED` either:
> without `prune`, on an adapter without `worker.prune`, it leaves the VM as it is and logs
> `<c>: kept VM <vm_id>: <adapter> <version> cannot keep its data (no worker.prune)`.

## 5. Compose

### 5.1 Scripts

Compose spawns a VM container's scripts as it spawns any script, with `III_CALL_DEFAULTS`. It
reads `BUILD_FAILED` from the call's one-line `Error:` instead of the
`iii sandbox: build failed:` line.

### 5.2 `build.json`

Compose records `build.json` once `scripts.build` exits 0, for a VM build as for a host build,
with the source read before and after the build (`BUILD_INPUT_CHANGED`). `image.build_id` comes
from `image.id`, `adapter` and `contract` from Compose's own `adapter::info`. The adapter's
`base_image.reference` and `digest` are in the call's answer, which Compose does not read:
`reference` records the image that was asked for, the declared one or the manifest's, and `digest`
is `null`. Nothing reads them: the out-of-date check compares the declared image only. What is lost
is the record of the digest a moving tag such as `:latest` resolved to; an adapter that writes it
next to the image (a contract follow-up) would bring it back.

### 5.3 Operators

- **Rebuild:** `iii call compose::build worker=<c>`. `compose::build` answers with an `output`
  stream of its progress and build lines. The stream ends when the operation does, and is aborted
  when it fails or is cancelled. A reader that leaves (Ctrl+C) cancels the operation. This replaces
  `iii sandbox build <container>` outside Compose.
- **Destroy:** `iii call compose::remove worker=<c>`, with `prune=true` to delete the data. This
  replaces `iii sandbox destroy`.

> **As built: follow-up.** The `output` stream of `compose::build` is not implemented yet. Until it
> is, `compose::build` stays an asynchronous operation that answers at once: follow the build with
> `iii compose logs <c> --follow`, where each build line has the `[build] ` prefix, or with the
> operation's `compose::on-operation-progress` events.

### 5.4 Removed

The `iii sandbox` subcommand, its CLI reference, its docs, and the CLI half of `sandbox_cli.rs`.
The rest moves to the router (4.1).

## 6. Delivery

1. `iii call` (section 3): its own pull request into `0.27.0`, then `0.27.0` is merged into the
   explicit VM mode branch.
2. The router (section 4): a port of `sandbox_cli.rs` with its tests.
3. Compose (section 5), the rewrite of section 2, and `compose_sandbox_e2e` on the new forms.
4. Docs: Compose, sandboxes, the upgrade guide, the CLI reference; the explicit VM mode spec points
   here.

The adapter in `iii-hq/workers` does not change.

## 7. Decisions

1. **Names:** `sandbox::build` and `sandbox::start`. `sandbox::run` stays the one-shot snippet
   run.
2. **Context:** per-function defaults in `III_CALL_DEFAULTS`, with `worker=` on the line and
   checked by Compose. No flag on the line, and no secret in the arguments.
3. **Exit code:** a VM that fails makes `iii call` exit 1 (`WORKER_EXITED`), with no
   `--exit-code` flag.
4. **Base image digest:** `null` in `build.json`.
5. **`iii call`:** the streaming SDK work is in `0.27.0` (#2561); what section 3 adds to the CLI is
   a separate pull request into `0.27.0`.
6. **A dead caller:** its VM is stopped, as on a SIGTERM, rather than left to the adapter's lease.
   A restart that reattaches to it while it stops boots it again once, so a crash costs the
   restart policy one attempt.
