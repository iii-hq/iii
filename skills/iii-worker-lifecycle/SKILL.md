---
name: iii-worker-lifecycle
description: >-
  Manage iii workers with the iii worker CLI. Use when adding, removing,
  starting, stopping, updating, inspecting, logging, executing into, or
  scaffolding registry, binary, OCI, or local managed workers.
---

# Worker Lifecycle

Use this skill when the task is about the `iii worker ...` command surface or managed worker lifecycle.

## Command Groups

| Command | Use when |
| --- | --- |
| `iii worker add <worker[@version]>` | Add a registry-managed worker and update config/lockfile when the resolver returns a graph |
| `iii worker add <oci-ref>` | Add an OCI/container worker such as `ghcr.io/org/worker:tag` |
| `iii worker add ./path` | Register a local worker project for managed sandbox/local development |
| `iii worker remove <name>` | Remove a worker from config and managed lifecycle |
| `iii worker reinstall <name>` | Reinstall a managed worker artifact without changing intent |
| `iii worker update [name]` | Intentionally re-resolve worker pins and rewrite `iii.lock` |
| `iii worker clear [name]` | Clear local managed-worker artifacts/cache before reinstalling or re-adding |
| `iii worker start <name>` | Start a managed worker that is configured but not running |
| `iii worker stop <name>` | Stop a running managed worker |
| `iii worker restart <name>` | Stop then start a managed worker |
| `iii worker list` | Show configured/managed workers |
| `iii worker status [name]` | Inspect runtime status, PID/state, and whether a worker is running |
| `iii worker logs <name>` | Read managed worker logs |
| `iii worker logs <name> --follow` | Tail logs while debugging startup/runtime issues |
| `iii worker exec <name> -- <cmd>` | Run a command inside a running worker environment |
| `iii worker init` | Scaffold worker metadata such as `iii.worker.yaml` for a local worker |
| `iii worker sync` | Replay registry-managed workers from `iii.lock` for the current platform |
| `iii worker sync --frozen` | Verify reproducibility without changing local files |
| `iii worker verify` | Check that config and `iii.lock` agree |

## Lifecycle Decisions

- Add a built-in or registry worker by name: `iii worker add iii-state`, `iii worker add image-resize@1.0.0`.
- Add multiple dependency workers in one command when docs show that pattern: `iii worker add iii-state iii-stream iii-queue`.
- Add a container worker by OCI reference when the worker ships as an image: `iii worker add ghcr.io/my-org/my-worker:latest`.
- Add a local worker path for development: `iii worker add ./workers/my-worker`.
- Use `update` only when intentionally accepting new resolved versions.
- Use `sync` after clone/CI restore to install artifacts from `iii.lock`.
- Use `verify` in CI before running the engine.
- Use `reinstall` or `clear` when the local managed artifact is corrupt or dependency setup changed.

## Local vs Registry-Managed

- Registry-managed workers are reproducible through `iii.lock` and `iii worker sync`.
- Local path workers are for development and may have local setup/cache state.
- Built-in workers can be referenced directly in config and may be skipped by lockfile verification.
- Registry pages may label core capabilities as `engine` workers. Still use the shown `iii worker add <name>` command when instructing users.
- Direct OCI refs are pinned best by immutable tags or digests; lockfile replay focuses on registry-managed worker graphs.

## Binary vs OCI Workers

- Binary workers resolve to platform-specific artifacts. `iii.lock` records artifacts by target triple.
- OCI/container workers run from images and can expose default config through image environment declarations.
- Managed sandbox/local workers run in isolated local environments where supported and are controlled by the same lifecycle commands.

## `iii worker exec`

Use `exec` like `docker exec` for managed workers:

```bash
iii worker exec image-resize -- ls -la /workspace
iii worker exec image-resize -e LOG_LEVEL=debug -w /tmp -- ./probe
iii worker exec image-resize --timeout 30s -- ./long-task
iii worker exec image-resize -- sh
```

TTY is auto-detected for interactive terminals. Use `-t` to force TTY and `--no-tty` for byte-exact pipe mode.

## Pattern Boundaries

- For lockfile reproducibility, prefer `iii-worker-lockfile`.
- For engine YAML fields, prefer `iii-engine-config`.
- For choosing which worker provides which capability, prefer `iii-worker-catalog`.
- For worker RBAC and public browser access, prefer `iii-worker-rbac`.

## When to Use

- Use this skill when the task mentions `iii worker` commands, worker installation, worker logs, worker status, worker exec, local worker development, OCI workers, or managed worker lifecycle.

## Boundaries

- Never use old aliases such as `iii install`, `iii uninstall`, or `iii list`; the current surface is `iii worker add/remove/list`.
- Do not generate removed service APIs or adapter-extension APIs.
- Always keep generated examples aligned with `config.yaml` plus `iii.lock` when registry-managed workers are involved.
