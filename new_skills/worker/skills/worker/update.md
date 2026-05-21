---
type: how-to
function_id: worker::update
title: Re-resolve registry versions and rewrite iii.lock
---

# When to use

Call `worker::update` to re-resolve each named worker against the
registry, download newer artifacts if available, and rewrite
`iii.lock`. Configs in `iii.config.yaml` are preserved — only the
pinned versions move. The call is idempotent: a second update with no
upstream change returns an empty `updated` list.

Reach for it when:

- A registry-tracked worker has a new release you want to pick up.
- You're maintaining a project and want every worker re-pinned to the
  latest published semver.
- A scheduled job runs this op periodically to keep the engine current.

Use [`worker::add`](iii://worker/add) instead when the worker isn't yet
installed (update only operates on existing entries) or when you want
to pin to a specific version rather than chasing latest.

# Inputs

```json
{
  "names": []  // explicit list of worker names; empty means "every installed registry-backed worker"
}
```

`names: []` updates every installed registry-backed worker (workers
added via the OCI source are skipped because they have no semver to
re-resolve). Names not present in `iii.config.yaml` return **W110**.

# Outputs

```json
{
  "updated": [
    {
      "name":          "image-resize",  // worker name as it appears in iii.config.yaml
      "from_version":  "0.1.2",         // pre-update version pinned in iii.lock
      "to_version":    "0.1.3"          // newly resolved version written to iii.lock
    }
  ]
}
```

- `updated` lists one entry per worker that actually changed version;
  workers already at latest are **omitted** rather than returned with
  matching `from_version` / `to_version`. An empty list means everything
  was already current.
- Rows are sorted lexicographically by `name`.
- `from_version` reflects the pre-call `iii.lock` value; `to_version`
  reflects the new pin and matches what a follow-up `worker::list` will
  surface.

# Side effects

- Downloads new artifacts into `~/.iii/managed/{name}/` for every worker
  whose resolved version changes.
- Rewrites `iii.lock` with the new resolved versions (existing pins for
  unchanged workers are preserved verbatim).
- Does **not** touch `iii.config.yaml` — configs stay exactly as
  written.

# Worked example

Update every installed registry-backed worker:

```json
{ "names": [] }
```

A no-op when every worker is already at latest:

```json
{ "updated": [] }
```

A real change when one worker had a newer release waiting:

```json
{ "updated": [{ "name": "image-resize", "from_version": "0.1.2", "to_version": "0.1.3" }] }
```

# Related

- `worker::add` — install a worker first (update only operates on
  existing entries).
- `worker::list` — verify the new versions landed in `iii.lock`.
- `worker::start` — restart workers that picked up a new version (no
  hot-reload).

## Errors

- **W110** name not installed.
- **W900** registry / network / filesystem failure.
