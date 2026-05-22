---
type: index
title: worker
---

# worker

Install, run, and uninstall workers owned by the `iii-worker-ops` daemon
(auto-spawned as an engine sidecar). Every op is also callable as
`iii worker <cmd>` on the CLI; the trigger surface documented here is the
SDK path that other workers and the engine itself use. The daemon both
**serves** `worker::*` calls and **publishes** lifecycle events to the
[`worker`](iii://worker/events) custom trigger type so other workers
can subscribe to install/remove/start/stop transitions without polling.

`worker::add` is the single entry point for getting a worker on disk:
registry slugs and full OCI refs both flow through it, write the entry to
`iii.config.yaml`, cache the artifact under `~/.iii/managed/{name}/`, and
pin the resolved version in `iii.lock`. Destructive ops
(`worker::remove`, `worker::stop`, `worker::clear`) require an explicit
`yes: true` to avoid silently dropping work.

All ops return errors as
`{ "type": "WorkerOpError", "code": "Wxxx", "message": "...", "details": {...} }`.
The recurring codes are **W100** InvalidName (`[a-z0-9_-]{1,64}`),
**W101** InvalidSource, **W102** LocalPathNotAllowedViaTrigger
(`kind: "local"` is CLI-only), **W103** MissingTarget (empty `names` +
`all` unset, or both set), **W104** ConsentRequired (destructive op needs
`yes: true`), **W110** NotFound, and **W900** Internal. The `W` codes are
independent of the `S` codes used by the [`sandbox::*`](iii://sandbox)
surface.

- **Worker** (`worker::*`) — install, run, and inspect lifecycle for
  every worker connected to this engine.
- **Worker trigger type** (`worker`) — subscribe to lifecycle events
  emitted by every `worker::*` op.

## How-tos

### worker (trigger type)

- [`worker`](iii://worker/events) — subscribe to lifecycle events emitted by every `worker::*` op (install, remove, start, stop, etc.). Filter by `operations` / `stages` / `workers`.

### `worker::*`

- [`worker::add`](iii://worker/add) — install from a registry slug or OCI ref; writes config + lock + cache.
- [`worker::remove`](iii://worker/remove) — drop entries from `iii.config.yaml` (consent required); leaves the cache untouched.
- [`worker::update`](iii://worker/update) — re-resolve registry versions and rewrite `iii.lock`.
- [`worker::start`](iii://worker/start) — spawn a configured worker and wait for the ready signal.
- [`worker::stop`](iii://worker/stop) — gracefully shut down a running worker (consent required).
- [`worker::list`](iii://worker/list) — read installed entries, run state, and pinned versions in one shot.
- [`worker::clear`](iii://worker/clear) — wipe cached artifacts under `~/.iii/managed/{name}/` (consent required); leaves the config untouched.
- [`worker::schema`](iii://worker/schema) — fetch the full JSON Schema for every op (plus `default_timeout_ms` and `idempotent` hints).
