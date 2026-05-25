---
name: iii-worker-lockfile
description: >-
  Reproduce registry-managed iii worker installs with iii.lock. Use when
  working on CI, teams, deployments, worker pinning, sync, frozen installs,
  verification, or config.yaml and lockfile consistency.
---

# Worker Lockfile

`iii.lock` is a YAML lockfile in the project root. It records resolved registry-managed worker sources so installs can be replayed across machines and CI.

## What It Records

- Lockfile version.
- Registry-managed worker entries.
- Binary source artifacts keyed by platform target triple.
- OCI/image source pins such as resolved image digests where available.
- Dependency graph pins returned by the worker registry.

`config.yaml` expresses intent: which workers the project uses. `iii.lock` records the resolved worker graph and artifacts needed to reproduce that intent.

## Commands

| Command | Mutates? | Behavior |
| --- | --- | --- |
| `iii worker add <worker[@version]>` | Yes | Adds a registry-managed worker and merges resolved entries into `iii.lock` when the registry returns a graph |
| `iii worker update [worker]` | Yes | Re-resolves latest for one locked worker, or inferred roots when no worker is given, then rewrites `iii.lock` |
| `iii worker sync` | Local artifacts only | Replays registry-managed workers from `iii.lock` for the current target without rewriting config or lockfile |
| `iii worker sync --frozen` | No | Delegates to verification; use in CI or read-only checks |
| `iii worker verify` | No | Checks that lockfile-managed workers in config have lock entries and artifacts for the current target |
| `iii worker verify --strict` | No | Adds dependency freshness checks for locked ranges and local `iii.worker.yaml` dependency blocks |

## Reproducible Install Workflow

```bash
iii worker add image-resize@1.0.0
git add config.yaml iii.lock
iii worker verify
```

After cloning or restoring CI state:

```bash
iii worker sync
iii worker verify
```

For CI checks that must not mutate:

```bash
iii worker sync --frozen
iii worker verify --strict
```

## Commit Rules

- Commit `iii.lock` with `config.yaml`.
- Update pins intentionally with `iii worker update`, not by hand-editing resolved artifact entries.
- Do not expect built-ins, direct OCI refs, local-path workers, or sandbox rootfs/base images to be replayed by the v1 lockfile contract.
- Extra lockfile entries are allowed by normal verification; strict verification adds freshness checks.

## Pattern Boundaries

- For command-by-command worker operations, prefer `iii-worker-lifecycle`.
- For config YAML structure, prefer `iii-engine-config`.

## When to Use

- Use this skill when the task mentions `iii.lock`, reproducible worker installs, frozen sync, worker verification, CI worker setup, or cross-machine worker reproducibility.

## Boundaries

- Do not invent lockfile entries manually.
- Do not treat `sync` as an update command; it replays existing pins.
- Do not generate removed service APIs or adapter-extension APIs.
