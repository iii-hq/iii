---
type: how-to
function_id: sandbox::create
title: Spin up a new sandbox VM from a catalog image
---

# When to use

Call `sandbox::create` to pull a catalog image and boot a fresh microVM,
returning a UUID `sandbox_id` you reuse for every other op on the
`sandbox::*` surface. The daemon only boots images present in its catalog
(presets `python` / `node` plus operator-added custom images in
`iii.config.yaml`); an empty catalog with no presets denies every call as a
deliberate fail-closed default.

Reach for it when:

- You need a fresh, isolated rootfs for running an LLM-generated patch
  against an untrusted codebase.
- A build worker or integration-test fixture wants a clean filesystem with
  a known toolchain.
- You're orchestrating a multi-step exec flow and want a stable
  `sandbox_id` to target across calls.

Use [`sandbox::list`](iii://sandbox/list) instead when you only want to
know which sandboxes are already running.

# Inputs

```json
{
  "image":              "ghcr.io/iii-hq/node:latest",  // required; preset (python/node) or full OCI ref in the catalog
  "cpus":               2,                             // optional vCPU count; daemon default applies when omitted
  "memory_mb":          1024,                          // optional RAM budget in MiB; daemon default applies when omitted
  "name":               "build-shard-3",               // optional human label, surfaced verbatim in sandbox::list
  "network":            "bridge",                      // optional networking mode; daemon default applies when omitted
  "idle_timeout_secs":  900,                           // optional auto-stop window after the last exec; 0 disables
  "env":                ["NODE_ENV=production"]        // Vec<String> of "K=V" entries, NOT a map
}
```

`image` is the only required field; everything else falls back to the
daemon defaults. There is no `cwd` field — the workdir is fixed by the
image's rootfs. Malformed input returns **S001**; an unknown `image` not
in the catalog returns **S100**.

# Outputs

```json
{
  "sandbox_id": "550e8400-e29b-41d4-a716-446655440000",  // UUID; reuse for sandbox::exec / sandbox::list / sandbox::stop
  "image":      "ghcr.io/iii-hq/node:latest"             // echoes the resolved image (preset names expand to OCI refs)
}
```

- `sandbox_id` is always a canonical UUID (not the `sbx_*` opaque shape
  some other engines use).
- `image` reflects the resolved OCI ref, so callers can record the exact
  build that booted even when they passed a preset name.

# Side effects

- Allocates a microVM and its rootfs under the daemon's runtime directory
  (disk footprint scales with the chosen image).
- Adds a row to the in-process sandbox registry that the next
  `sandbox::list` will surface.
- May trigger an auto-install of the underlying rootfs if missing
  (transient **S102** failures are retryable).

# Worked example

Boot a node image with two cores, 1 GiB of RAM, and a single env var:

```json
{
  "image":     "ghcr.io/iii-hq/node:latest",
  "env":       ["NODE_ENV=production"],
  "cpus":      2,
  "memory_mb": 1024
}
```

Returns the `sandbox_id` you then pass to
[`sandbox::exec`](iii://sandbox/exec) for each command, finishing with a
[`sandbox::stop`](iii://sandbox/stop) when the work is done.

# Related

- `sandbox::exec` — run commands inside the sandbox you just created.
- `sandbox::list` — verify the new sandbox is registered and check its state.
- `sandbox::stop` — tear the sandbox down once the work is finished.

## Errors

- **S001** invalid request (malformed `image`, missing required field).
- **S100** image not in catalog. Add it to the catalog or use a preset.
- **S101** rootfs missing on disk — run `iii worker add <image-ref>` first.
- **S102** auto-install failed (transient — retry).
- **S300** VM boot failed.
- **S400** resource limit hit (too many concurrent sandboxes).
