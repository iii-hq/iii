---
type: index
title: sandbox
---

# sandbox

Ephemeral microVMs owned by the `iii-sandbox` daemon. Each sandbox is scoped
to the engine it runs inside; reach for them to execute an LLM-generated
patch against an untrusted codebase, spin up an ephemeral build worker, or
provision an integration-test fixture.

`sandbox::create` rejects any image not in the daemon's catalog (presets
`python` / `node`, plus operator-added custom images in `iii.config.yaml`).
An empty catalog with no presets denies every call — a deliberate
fail-closed default. The catalog is the only authority on what can boot.

Every op returns errors as
`{ "type": "<category>", "code": "Sxxx", "message": "...", "docs_url": "...", "retryable": bool }`,
where `type` is one of `validation`, `config`, `internal`, `transient`,
`execution`, `filesystem`, `platform` — not the literal string
`"SandboxError"`. The `S` codes are independent of the `W` codes used by
the [`worker::*`](iii://worker) surface.

- **Sandbox** (`sandbox::*`) — VM lifecycle: create, exec, list, stop.

## How-tos

### `sandbox::*`

- [`sandbox::create`](iii://sandbox/create) — pull a catalog image and boot a fresh sandbox; returns the UUID you'll reuse for every other op.
- [`sandbox::exec`](iii://sandbox/exec) — run one command inside a running sandbox; serialized per `sandbox_id`.
- [`sandbox::list`](iii://sandbox/list) — read the current sandbox roster (safe to poll).
- [`sandbox::stop`](iii://sandbox/stop) — signal a sandbox to shut down and reap its resources.
