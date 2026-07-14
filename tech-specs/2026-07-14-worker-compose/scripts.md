# scripts: pre_start / run / post_run

Per-container scripts in the **compose file**. Three hooks, one supervised
process, no stop-hooks: teardown stays signal-based (SIGTERM → grace →
SIGKILL on the process group).

## Contract

| Hook | Blocking | When | On failure |
| --- | --- | --- | --- |
| `pre_start` | yes | every time compose is about to spawn the worker; after config resolution, before spawn | exit ≠ 0 or timeout → container `failed`, worker never spawns, `up` rolls back what this operation started |
| `run` | supervised | the worker process itself | crash → cascading stop of local dependents (see lifecycle.md) |
| `post_run` | no | once, after the `run` process **exits** — any exit path (down, post-ready crash, `up` rollback) | warning + `last_error` in status; teardown proceeds without waiting |

- `pre_start_timeout`: default **60s**; set the field only to change it.
  Timeout kills the hook's process group.
- Hooks run for **any** worker type (they execute on the daemon's host).
  `run` is only meaningful for `path://` — on `package://` binaries the start
  is implicit: exec the resolved artifact with the standard CLI contract
  (`--url`, `--namespace`, `--config`).
- Precedence for `path://`: `run` in compose **>** `scripts.start` in
  `iii.worker.yaml`. With neither → validation error ("add run: or create
  iii.worker.yaml").
- `setup` / `install` from the manifest are **not executed by compose** — they
  remain the sandbox/registry contract. Dependency install is the developer's
  job or the `pre_start`'s.

## Execution context

- env: the container's resolved env — the same the `run` gets (host env +
  injected url/namespace/config). A migration in `pre_start` sees the same
  database the worker will see.
- cwd: the container's `working_dir`.
- shell: `sh -c` on unix, `cmd /C` on windows; single string, no array form.
- isolation: each hook in its own process group / Job Object — killing a hook
  never touches the worker; stdout/stderr land in the container log with a
  phase prefix (`[pre_start]`, `[post_run]`), visible through
  `compose::logs id=<daemon>`.

## Order

```
resolve config  →  pre_start (blocking)  →  spawn run  →  readiness
(engine registration)  →  … worker lifetime …  →  run EXITS
(down, crash, or rollback)  →  post_run (fire, don't wait)
```

`post_run` is **not awaited by ordered teardown**: it fires once the process
exit is confirmed, and the dependent cascade and registration release proceed
without waiting for it. A dependent that needs preparation before its own
start puts that logic in its own `pre_start`.

## Why hooks in the compose file (and not only the manifest)

The 2026-07-13 review removed `scripts:` from compose to keep it worker-
focused. This proposal brings back a **scoped** version because two real
cases have nowhere else to live:

1. **project-level preparation** — `prisma migrate` belongs to the project
   composing the worker, not to the published worker package;
2. **manifest-less local workers** — the monorepo dev case (`run: pnpm dev`)
   should not require authoring a manifest first.

The original objection (precedence stacking, compose bloat) is addressed by
the cut: three hooks, `run` only for local workers, no `setup`/`install`
override, single explicit precedence rule.

## Deferred

a `stop` hook (custom graceful-stop command — teardown stays signal-based;
`post_run` already covers after-exit), array-form commands, project-level
(non-container) hooks, hooks inside the sandbox path.
