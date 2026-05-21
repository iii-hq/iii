---
type: how-to
function_id: worker::add
title: Install a worker from the registry or an OCI image
---

# When to use

Call `worker::add` to install a worker. It writes the entry to
`iii.config.yaml`, caches the artifact under `~/.iii/managed/{name}/`,
and pins the resolved version in `iii.lock`. The call is idempotent —
calling twice with the same `source` yields the same outcome, distinguished
only by the `status` field on the response.

Reach for it when:

- A new project needs a worker on disk and registered with the engine.
- A cache has gone stale and you want to force a clean re-download
  (`force: true`).
- A different version of the same worker is installed and you want
  `iii.lock` re-pinned to the new one.

Use [`worker::update`](iii://worker/update) instead when the worker is
already installed and you only want to re-resolve the latest registry
version (no fresh install needed).

# Inputs

```json
{
  "source":        { "kind": "registry", "name": "image-resize", "version": "0.1.2" },  // required; one of the three variants below
  "force":         false,  // optional; true re-downloads even when iii.lock already matches
  "reset_config":  false,  // optional; true overwrites the worker's existing iii.config.yaml entry
  "wait":          true    // optional; true blocks until the worker reports ready (used by the daemon's health check)
}
```

`source` is the only required field; everything else has a safe default.
There are three valid `source` variants — registry, OCI ref, and local
path — and **only** the first two are accepted over the trigger surface.

Registry slug (optionally pinned to a semver):

```json
{ "kind": "registry", "name": "image-resize", "version": "0.1.2" }
```

Full OCI reference:

```json
{ "kind": "oci", "reference": "ghcr.io/iii-hq/node:latest" }
```

Local path — **CLI only**; the same shape over the trigger surface
returns **W102** so untrusted callers can't side-load arbitrary code:

```json
{ "kind": "local", "path": "./builds/image-resize" }
```

Missing or malformed `source` returns **W101**; a registry name that's
not published returns **W110**.

# Outputs

```json
{
  "name":           "image-resize",                                                   // resolved worker name (matches the registry slug or the OCI tag basename)
  "version":        "0.1.2",                                                          // resolved semver; omitted for OCI sources without a semver tag
  "status":         "installed",                                                      // one of: installed | already_current | repaired | replaced
  "awaited_ready":  true,                                                             // true when the call waited for the worker's ready signal (matches the input `wait`)
  "config_path":    "/Users/you/.iii/managed/image-resize/iii.config.yaml"            // absolute path to the worker's config file on disk
}
```

- `version` is omitted from the JSON for OCI sources whose tag isn't a
  semver (callers should treat absence as "untagged" rather than empty
  string).
- `status` distinguishes the four real outcomes: `installed` (new),
  `already_current` (lockfile match — no work done), `repaired` (cache
  was corrupt and got re-pulled), `replaced` (a different version was
  installed before this call).
- `config_path` is the absolute path you'd hand to a follow-up
  `worker::start { config: ... }` if you wanted to override the on-disk
  config.

# Side effects

- Writes (or rewrites) the worker's entry in `iii.config.yaml`.
- Downloads the artifact into `~/.iii/managed/{name}/` (creates the
  directory if it doesn't exist).
- Pins the resolved version in `iii.lock`, replacing any prior pin for
  the same name.
- Subsequent `worker::add` calls observe the new lock entry; pair with
  [`worker::clear`](iii://worker/clear) to reclaim disk after a
  `worker::remove`.

# Worked example

Pin a registry slug to an exact semver and wait for the worker to come up:

```json
{
  "source":        { "kind": "registry", "name": "image-resize", "version": "0.1.2" },
  "force":         false,
  "reset_config":  false,
  "wait":          true
}
```

Returns `status: "installed"` on a fresh install, `status: "already_current"`
on the second call with the same inputs, or `status: "replaced"` if a
different version was previously pinned.

# Related

- `worker::start` — spawn the worker once it's installed.
- `worker::update` — re-resolve the latest version after a previous add.
- `worker::remove` — drop the entry from `iii.config.yaml` (then
  `worker::clear` to free the cache).
- `worker::schema` — discover the exact field shapes before constructing
  the request.

## Errors

- **W101** missing/malformed `source`.
- **W102** local path via trigger.
- **W110** worker name not in registry.
- **W900** OCI pull, network, or filesystem failure (see `details.message`).
