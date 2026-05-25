---
type: how-to
function_id: worker::clear
title: Wipe cached worker artifacts from disk
---

# When to use

Call `worker::clear` to delete cached artifacts under
`~/.iii/managed/{name}/`. The worker's entry in `iii.config.yaml` and
its pin in `iii.lock` are **not** touched — call
[`worker::remove`](iii://worker/remove) for that. The op is destructive
and requires `yes: true` consent. It's idempotent: a second clear on
the same target returns `cleared_bytes: 0`.

Reach for it when:

- You just called [`worker::remove`](iii://worker/remove) and want to
  reclaim the disk space the cached artifact still occupies.
- A cache has gone bad and you want to force a clean re-download on
  the next [`worker::add`](iii://worker/add) (combine with
  `worker::add { force: true }` for a clean rebuild).
- A scripted cleanup is purging disk usage without altering the
  declared worker set.

Use [`worker::remove`](iii://worker/remove) instead when you want the
worker gone from `iii.config.yaml` (clear leaves the config untouched).

# Inputs

```json
{
  "names":  ["image-resize"],  // explicit list of worker names whose caches to wipe; mutually exclusive with `all`
  "all":    false,             // optional; true clears every cache under ~/.iii/managed/
  "yes":    true               // required consent flag; must be exactly true
}
```

Same target rules as [`worker::remove`](iii://worker/remove): pass
either `names` (a non-empty array) **or** `all: true`, never both,
never neither (**W103**). `yes` must be exactly `true` or the call
returns **W104**.

# Outputs

```json
{
  "cleared_bytes":  812727797  // total bytes freed across every cleared cache; 0 when nothing matched
}
```

- `cleared_bytes` is the sum across every cleared cache directory; a
  no-op (everything already empty) returns `0`.
- Names that don't have a cache directory on disk are silently
  skipped — the total reflects only the real deletes.

# Side effects

- Recursively deletes `~/.iii/managed/{name}/` for each matched name
  (every artifact, every config file the daemon copied there).
- Does **not** mutate `iii.config.yaml` or `iii.lock` — the worker
  entry and pinned version stay exactly as they were.
- After this call, the next [`worker::add`](iii://worker/add) (or a
  start that needs the cache) will re-download the artifact.
- Publishes lifecycle events to the [`worker`](iii://worker/events)
  trigger type: `started` → `clearing` → `done` on success, or
  `started` → `clearing` → `failed` on error. Subscribers filter via
  `WorkerTriggerConfig`.

# Worked example

Wipe a single worker's cache:

```json
{ "names": ["image-resize"], "yes": true }
```

Or wipe every cache (full disk reset):

```json
{ "all": true, "yes": true }
```

Returns the total bytes freed:

```json
{ "cleared_bytes": 812727797 }
```

# Related

- `worker::remove` — drop the config entry first when fully
  decommissioning a worker.
- `worker::add` — re-download into a fresh cache (pair with
  `force: true` to guarantee a clean pull).
- `worker::list` — confirm the worker entries are still configured
  after the clear.
- [`worker`](iii://worker/events) — subscribe to lifecycle events fired
  by this op.

## Errors

- **W103** target missing or ambiguous.
- **W104** consent missing.
- **W900** filesystem failure (rare).
