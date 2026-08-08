---
title: worker compose — distributed worker lifecycle
tagline: one daemon per machine supervises the workers a compose file declares — namespaced, config-fed, crash-cascading, engine-coordinated.
date: 2026-07-14
tags: [compose, workers, lifecycle, dx]
status: draft
---

# Worker Compose

A standalone **compose daemon** that supervises iii workers declared in one
`worker-compose.yaml`. The daemon is not the engine: `iii compose` starts only
the daemon, which connects to an engine like any worker, registers the
`compose::up/down/list/status/logs/validate` control functions, and owns
exactly the child processes it spawned on its own machine. Several daemons —
several machines — attach to one engine; a trigger addresses one of them by
argument: `iii trigger compose::up id=host-a`.

The load-bearing separation: **the engine routes and arbitrates; the daemon
supervises.** Worker lifecycle today is welded to the machine the engine runs
on (`iii worker` + iii-worker-ops). Compose breaks that weld so workers can
run where their resources are, while the engine stays the single coordination
point: it rejects duplicate registrations inside a namespace, holds triggers
for workers that have not arrived yet, and answers "who is running what".

## What this pack proposes

Six contracts, each in its own doc:

| Doc | Contract |
| --- | --- |
| [compose-file.md](./compose-file.md) | `worker-compose.yaml` v1 — an id-keyed `containers:` object; `depends_on` inside one file only |
| [scripts.md](./scripts.md) | per-container `scripts:` — `pre_start` (blocking, timed), `run` (supervised), `post_run` (fires after the run exits) |
| [namespace.md](./namespace.md) | namespace as a runtime argument — never a function-name prefix; engine rejects same-name collisions inside a namespace |
| [configuration.md](./configuration.md) | the daemon fetches base config, applies compose overrides, hands the worker its final config at start |
| [cli-contract.md](./cli-contract.md) | one CLI/env contract for every worker binary: `--url`, `--namespace`, `--config`; `registerWorker()` reads env |
| [lifecycle.md](./lifecycle.md) | up/down over a validated DAG, readiness = engine registration, cascading failure, optimistic trigger buffer |

## Why now

Three facts from the current codebase force each major piece:

1. **Duplicate registration is a silent overwrite.** The engine warns and
   replaces on a name collision (`engine/src/services.rs:106-111` — and a unit
   test codifies it: `registry_insert_service_duplicate_overwrites`). Running
   two state workers means the last one wins and the first keeps running
   blind. Namespaces + hard rejection replace that with a deterministic error.
2. **No two worker binaries agree on how to find the engine.** `http`, `state`,
   `storage`, `database` take only a `--url` flag with a hardcoded
   `ws://127.0.0.1:49134` default and read no env; `llm-router` reads
   `III_WS_URL`; `shell` and friends read `III_URL`. No SDK reads any address
   env at all. A supervisor cannot inject a contract that does not exist —
   the CLI/env standard creates it.
3. **Config delivery is first-boot-only.** A worker's `--config` seeds the
   configuration entry once; afterwards "the configuration worker is the
   authoritative source and `--config` is ignored" (`workers/http/src/main.rs`
   doc comment). Compose-managed config therefore cannot be a file passed at
   spawn — the daemon has to resolve base + overrides and deliver the final
   value through the standard contract.

## Principles (decided 2026-07-13, team review)

- **compose over docker**: bare-metal child processes locally; the sandbox
  path stays the validated route to cloud. Docker is not the default runtime.
- **fail early over silent overwrite**: same name + same namespace = rejected
  connection, not a zombie.
- **one file, one dependency scope**: `depends_on` never crosses compose
  files. Sharing across files is a namespace decision, not a dependency edge.
- **worker knowledge stays with the worker**: compose does not carry full
  configurations, and `setup`/`install` remain the worker/sandbox contract.
- **zero-code onboarding**: `registerWorker()` with no arguments must work —
  the daemon injects url/namespace/config through env; developers learn
  namespaces only when they need two of something.

## Honest trade-offs

- The namespace model requires protocol + all-three-SDK changes
  (register/trigger carry a namespace; env contract) — the widest surface in
  the pack.
- `run` in the compose file partially reverses the 2026-07-13 "no scripts in
  compose" decision; the counter-argument is scoped: three hooks, `run` only
  for local workers, no setup/install override.
- Per-target artifacts (a real release ships 9 platform binaries with 9
  digests) mean reproducibility metadata must be per-target from day one.
- The optimistic trigger buffer trades "fail loud on missing worker" for
  "wait with a bounded timeout" — the timeout knob is what keeps that honest.
