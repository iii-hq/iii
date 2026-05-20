---
type: index
title: iii-exec
---

# iii-exec

Run a sequential pipeline of shell commands as part of engine startup. Each entry in the configured `exec:` array launches as a separate child process, must exit `0` before the next entry starts, and the **final** entry is kept running as a long-lived process for the lifetime of the engine. Optionally, a `watch:` glob list restarts the entire pipeline whenever a matching file changes.

The worker exposes **no callable functions and no trigger types**. Its surface is entirely declarative — configure `exec:` in `iii-config.yaml` and the engine handles the rest. There is nothing for an agent to call at runtime; this skill describes when to reach for the worker and how to shape its config.

For the full config schema (`exec:`, `watch:`), the per-process semantics, and the watch-pattern limitations (root-dir + extension matching only; filename portion ignored; files without extensions never matched), see [the README](../README.md).

- **Startup pipeline** (`exec:`) — sequential commands that must succeed in order, with the final command kept running as a long-lived process bound to the engine's lifecycle.
- **File-watch restart** (`watch:`) — glob patterns that trigger a full pipeline restart on change. Useful for development; rarely the right choice for production.

## How-tos

### When to use `iii-exec`

Reach for this worker when the engine should manage the lifecycle of a separate process — typically a long-running app or daemon that the engine boots, supervises, and shuts down alongside itself. Common patterns:

- **Build then serve.** Two prep commands that must succeed before the long-lived server runs:
  ```yaml
  - name: iii-exec
    config:
      exec:
        - cd frontend && npm install
        - cd frontend && npm run build
        - bun run --enable-source-maps index-production.js
  ```
  The first two run sequentially and must exit `0`; the third stays running. If any prep step fails (non-zero exit), the pipeline stops, the long-lived server never starts, and the engine surfaces the failure.

- **Development with file watch.** Add a `watch:` block so the pipeline restarts when source files change:
  ```yaml
  - name: iii-exec
    config:
      watch:
        - steps/**/*.{ts,js}
        - config/*.json
      exec:
        - cd frontend && npm install
        - cd frontend && npm run build
        - bun run --enable-source-maps index-production.js
  ```
  Note from the [README](../README.md): watch patterns match by root directory + file extension only — the filename portion is ignored. `src/**/*.test.ts` matches every `.ts` file under `src/`, not only `*.test.ts`. Files without an extension are never matched.

### When *not* to use `iii-exec`

- **Recurring scheduled jobs.** Use the `cron` reactive trigger from `iii-cron` (see its [skills bundle](https://github.com/iii-hq/iii/tree/main/engine/src/workers/cron/skills)) — `iii-exec` is for one-shot startup pipelines and a single long-lived process, not for cron-style scheduling.
- **Inter-process state sharing across entries.** Each entry runs in its own shell, so working directory and environment changes do not carry forward. Chain steps that need to share state with `&&` inside one entry:
  ```yaml
  - cd path/to/project && npm run build
  ```
- **Calling shell commands at runtime from a function.** `iii-exec` runs at engine startup only. There's no engine function for "execute this command now."

### Operational notes

- **Sequential execution, not parallel.** Entries run in order. Use a separate process supervisor (or multiple `iii-exec` worker instances if your config supports them) when you genuinely need parallel processes.
- **Exit-code propagation.** Non-zero exit on any intermediate command stops the pipeline; remaining commands are skipped. The final long-lived command's exit signals engine shutdown of this worker.
- **Watch caveats.** The watcher is intentionally simple — review [the README's note on pattern limitations](../README.md) before relying on watch in CI or production.

For the full schema and runnable examples, see [the README](../README.md) and [the iii main repo](https://github.com/iii-hq/iii).
