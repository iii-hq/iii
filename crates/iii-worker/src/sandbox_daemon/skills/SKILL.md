---
name: iii-sandbox
description: >-
  Ephemeral microVM sandboxes for running untrusted or agent-generated code in
  isolation — a one-call `sandbox::run`, a create/exec/stop lifecycle, and ten
  `sandbox::fs::*` file operations.
---

# iii-sandbox

The `iii-sandbox` worker boots ephemeral libkrun microVMs and runs code inside them, isolated from the host. Each sandbox boots in a few hundred milliseconds, runs commands or file operations scoped to the engine it lives on, and is reaped when idle; the overlay filesystem is discarded on stop. Callers reach it through sixteen `sandbox::*` functions invoked with `iii.trigger({ function_id: 'sandbox::...', payload })` (or `agent_trigger { function: 'sandbox::...' }` from the agent loop).

There are two ways in. `sandbox::run` is the fast path: it boots a VM, runs a code snippet, captures stdout/stderr, and tears the VM down in a single call. For multi-step work — several commands, multiple files, or inspecting a VM between steps — use the `sandbox::create` → `sandbox::exec` / `sandbox::fs::*` → `sandbox::stop` lifecycle and carry the returned `sandbox_id` across calls.

Prerequisites: the worker is enabled by adding `iii-sandbox` to `config.yaml`, and the host needs hardware virtualization (Apple Silicon, or `/dev/kvm` on Linux) — Intel Macs and Windows cannot boot sandboxes. Bootable images are catalog names gated by a fail-closed allowlist, not arbitrary OCI references.

## When to Use

- Execute untrusted or LLM-generated code without exposing the host.
- One-shot "code in, stdout out" — reach for `sandbox::run`.
- Spin up an ephemeral build worker, integration-test fixture, or throwaway iii worker.
- Read, write, search, or edit files inside an isolated filesystem.

## Boundaries

- Not for long-lived services or durable state — the overlay is wiped on stop. Use a regular worker for daemons and `iii-state` for persistence.
- `image` is a catalog name (`node`, `python`, or an operator-registered custom image), never an OCI ref — unknown names return `S100`; discover the live set with `sandbox::catalog::list`.
- `sandbox::exec` runs one command at a time per sandbox (serialized) — a concurrent call returns `S003`, and waiting does NOT free the slot when the in-flight command is a long-running or foreground process (a server, `npm install`, a build). Run those detached (`nohup … &`, or `sandbox::run` with `lang: "shell"`), or recover by replacing the sandbox with `sandbox::stop` then `sandbox::create`.
- `sandbox::exec` is not a shell — for pipes, `&&`, redirects, or variable expansion use `sandbox::run` with `lang: "shell"` or wrap in `sh -c`.
- Requires host virtualization; boot failures on unsupported hosts surface as `S300`.

## Functions

- `sandbox::run` — boot a VM, run a code snippet (`lang` selects the interpreter), capture output, and auto-stop, in one call.
- `sandbox::create` — boot a microVM from a catalog image; returns the `sandbox_id` every other op uses.
- `sandbox::exec` — run a single command inside a running sandbox.
- `sandbox::stop` — destroy a sandbox and reclaim its resources.
- `sandbox::list` — enumerate sandboxes currently running on this engine.
- `sandbox::catalog::list` — list bootable image names (bundled presets plus operator custom images).
- `sandbox::fs::write` — write a file into a sandbox; the body is exactly one of `content` (UTF-8 string), `content_b64` (base64 binary), or `content` as a `StreamChannelRef` for streaming uploads.
- `sandbox::fs::read` — read a file out of a sandbox.
- `sandbox::fs::ls` — list the entries of a directory.
- `sandbox::fs::stat` — read metadata for a single path.
- `sandbox::fs::mkdir` — create a directory.
- `sandbox::fs::rm` — remove a file or directory.
- `sandbox::fs::mv` — move or rename a path.
- `sandbox::fs::chmod` — change a path's permissions (and optionally owner).
- `sandbox::fs::grep` — recursive regex search across files.
- `sandbox::fs::sed` — regex find-and-replace across files.

The `env` field on `create`, `exec`, and `run` accepts both `["K=V"]` and `{ "K": "V" }` shapes. Every error is returned as JSON inside `error.message` carrying `code`/`type`/`retryable` and a self-healing `fix` payload — parse and read it before retrying. With `network: true`, `localhost`/`127.0.0.1` URLs in the boot `env` are rewritten to the per-sandbox gateway at create time so guest code can reach host services. For exact request and response shapes, call `iii get function info` on any `sandbox::*` function id.
