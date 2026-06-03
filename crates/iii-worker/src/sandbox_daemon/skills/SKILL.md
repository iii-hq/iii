---
name: iii-sandbox
description: >-
  Ephemeral microVM sandboxes for running untrusted or agent-generated code in
  isolation — a one-call run path, a create/exec/stop lifecycle, and a set of
  filesystem operations.
---

# iii-sandbox

The `iii-sandbox` worker boots ephemeral libkrun microVMs and runs code inside them, isolated from the host. Each sandbox boots in a few hundred milliseconds, runs commands or file operations scoped to the engine it lives on, and is reaped when idle; the overlay filesystem is discarded on stop. Its `sandbox::*` functions are called like any other iii function.

There are two ways in. `sandbox::run` is the fast path: it boots a VM, runs a code snippet, captures stdout/stderr, and tears the VM down in a single call. For multi-step work — several commands, multiple files, or inspecting a VM between steps — use the `sandbox::create` → `sandbox::exec` / `sandbox::fs::*` → `sandbox::stop` lifecycle and carry the returned sandbox id across calls.

Prerequisites: the worker is enabled by adding `iii-sandbox` to `config.yaml`, and the host needs hardware virtualization (Apple Silicon, or `/dev/kvm` on Linux) — Intel Macs and Windows cannot boot sandboxes.

## Get the contract from the engine

This page tells you WHEN and HOW to use the sandbox; the engine is the source of truth for every function's exact arguments and responses. Before you call any `sandbox::*` function, fetch its contract and build your payload from that — do not guess field names, types, or formats from memory:

```
engine::functions::info { function_id: "sandbox::<fn>" }
```

(e.g. `{ function_id: "sandbox::fs::write" }`). The returned schema is authoritative; this skill never restates it.

## Running an iii worker inside a sandbox

To author an iii worker (read the `iii` skill first), you write the worker code into the sandbox, install its deps, and run it as a process there — but the worker must still join the HOST engine bus to be useful.

- Reaching the host engine: enable networking when you create the sandbox (the field is in the `sandbox::create` contract — fetch it), AND set the worker's engine-URL env (`III_ENGINE_URL`, e.g. `ws://localhost:49134`) IN THE `sandbox::create` `env` — NOT later at exec/run time. The localhost→host rewrite happens ONLY for the create-time `env`: a `localhost` / `127.0.0.1` engine URL set there is rewritten to the per-sandbox gateway so the worker reaches the host bus. The same `localhost` URL passed at `sandbox::exec` time is NOT rewritten — it points at the sandbox's own loopback, the worker silently fails to connect, and you will waste turns debugging TCP. So: put the engine URL in the create `env`; the worker process inherits it already rewritten.
- Run it backgrounded, never as a foreground `exec`: starting the worker with `sandbox::exec "node index.mjs"` blocks and times out (`S200 exec timed out`), because the process never returns. Launch it detached (`… &`, `nohup`, or `sandbox::run` shell mode) so the exec call returns while the worker keeps running — see the serialized-exec boundary below.
- Find the result: once the worker connects, its functions appear in `engine::functions::list` and any `http` trigger it binds is served by the `iii-http` worker; verify endpoints with `web::fetch`, not `curl`.
- EPHEMERAL by nature: a worker hosted in a sandbox dies when the sandbox is stopped or reaped (the overlay is discarded). That is fine for a demo, test, or one-off. For a worker that must stay up, install it as a managed worker (`worker::add`, see the `iii` skill) instead of hosting it in a sandbox.

## When to Use

- Execute untrusted or LLM-generated code without exposing the host.
- One-shot "code in, stdout out" — reach for `sandbox::run`.
- Spin up an ephemeral build worker, integration-test fixture, or throwaway iii worker.
- Read, write, search, or edit files inside an isolated filesystem.

## Boundaries

- A sandbox is for running code and build steps, NOT for hosting a long-lived server. To expose an HTTP API or any always-on endpoint, author an iii worker and register a trigger for it (an `http` trigger is served by the `iii-http` worker) — do NOT start a server process (`express`, `http.createServer`, a framework `listen()`) inside the sandbox. A server you start here is not routed by iii, is unreachable as an iii endpoint, and as a foreground process hangs the exec slot until it times out. If the task is "build an iii worker," the deliverable is registered functions plus a trigger, not a running server.
- Not for long-lived services or durable state — the overlay is wiped on stop. Use a regular worker for daemons and `iii-state` for persistence.
- Bootable images are catalog NAMES (`node`, `python`, or an operator-registered custom image), never arbitrary OCI references. Discover the live set with `sandbox::catalog::list`; an unknown name fails fast.
- `sandbox::exec` runs one command at a time per sandbox (serialized). A concurrent call is rejected, and waiting does NOT free the slot when the in-flight command is long-running or foreground (a server, `npm install`, a build) — it holds the slot until it exits. Run those in the background, or use `sandbox::run` in shell mode, or recover by replacing the sandbox (`sandbox::stop` then `sandbox::create`).
- `sandbox::exec` is not a shell — for pipes, `&&`, redirects, or variable expansion use `sandbox::run` in shell mode, or wrap the command in `sh -c`.
- Requires host virtualization; boot failures on unsupported hosts are reported as errors, not silent hangs.
- Sandboxes are network-isolated by default; reaching host services is opt-in at create time — see `engine::functions::info` on `sandbox::create` for how to enable it.

## Functions

A map of what exists. For the arguments and response of any one, call `engine::functions::info { function_id: "<id>" }`.

- `sandbox::run` — boot a VM, run a code snippet, capture output, and auto-stop, in one call.
- `sandbox::create` — boot a microVM you then drive with the lifecycle ops; returns the id every other op uses.
- `sandbox::exec` — run a single command inside a running sandbox.
- `sandbox::stop` — destroy a sandbox and reclaim its resources.
- `sandbox::list` — enumerate sandboxes currently running on this engine.
- `sandbox::catalog::list` — list bootable image names (bundled presets plus operator custom images).
- `sandbox::fs::write` — write a file into a sandbox.
- `sandbox::fs::read` — read a file out of a sandbox.
- `sandbox::fs::ls` — list the entries of a directory.
- `sandbox::fs::stat` — read metadata for a single path.
- `sandbox::fs::mkdir` — create a directory.
- `sandbox::fs::rm` — remove a file or directory.
- `sandbox::fs::mv` — move or rename a path.
- `sandbox::fs::chmod` — change a path's permissions (and optionally owner).
- `sandbox::fs::grep` — recursive regex search across files.
- `sandbox::fs::sed` — regex find-and-replace across files.

Every error comes back structured with a machine-readable hint describing how to fix it — read it and apply the fix before retrying, rather than re-sending the same call.
