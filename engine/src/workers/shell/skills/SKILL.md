---
name: iii-exec
description: >-
  Run a sequential pipeline of shell commands at engine startup, keeping the
  final command alive for the engine's lifetime, with optional file-watch
  restarts. Configure it entirely in `iii-config.yaml`.
---

# iii-exec

The `iii-exec` worker runs a sequential pipeline of shell commands as part of engine startup. Each entry in the configured `exec:` array launches as a separate child process and must exit `0` before the next entry starts; the final entry is kept running as a long-lived process for the engine's lifetime. An optional `watch:` glob list restarts the whole pipeline whenever a matching file changes.

The worker exposes no callable functions and no trigger types — its surface is entirely declarative. There is nothing for an agent to call at runtime; configure `exec:` (and optionally `watch:`) on the worker and the engine manages the lifecycle, supervising and shutting the process down alongside itself.

## When to Use

- The engine should manage the lifecycle of a separate long-running app or daemon (boot, supervise, shut down together).
- You need prep steps that must succeed in order before a long-lived server starts (for example, install then build then serve).
- During development, the pipeline should restart automatically when source files change (via `watch:`).

## Boundaries

- Not a runtime command executor — the pipeline runs at engine startup only; there is no function to "run this command now."
- Not a scheduler — use the `iii-cron` trigger for recurring jobs; `iii-exec` is one-shot startup plus a single long-lived process.
- Entries run sequentially, not in parallel, and do not share working directory or environment; chain dependent steps with `&&` inside one entry.
- A non-zero exit on any intermediate command stops the pipeline and the long-lived command never starts.

## Configuration

- `exec: string[]` (required) — sequential pipeline of shell commands. Intermediate entries must exit `0`; the final entry is held open as the long-lived process.
- `watch: string[]` (optional) — glob patterns that restart the whole pipeline on file change. Caveat: the watcher matches by root directory plus file extension only — the filename portion is ignored, so `src/**/*.test.ts` matches every `.ts` file under `src/`, not just test files. Files without an extension are never matched. Use `**` for recursive depth (`config/*.json` watches only the top level; `config/**/*.json` watches at any depth).

```yaml
- name: iii-exec
  config:
    watch:
      - steps/**/*.{ts,js}
    exec:
      - cd frontend && npm install
      - cd frontend && npm run build
      - bun run --enable-source-maps index-production.js
```

The first two commands run in order and must exit `0`; the third stays running for the worker's lifetime. If a prep step fails, the pipeline stops and the engine surfaces the failure.
