# CLARIFICATION — Sandbox worker name

**Blocker for:** [`WORK.workers.md`](./WORK.workers.md) (per-worker checklists),
[`WORK.cli.md`](./WORK.cli.md) §4 (sandbox subcommand removal), [`WORK.engine.md`](./WORK.engine.md)
§1 (sandbox CLI removal).

## What we know

- The `iii sandbox` subcommand is being removed and replaced by `iii trigger sandbox::*` invocations
  (per the user during this session).
- Functions to expose: `sandbox::run`, `sandbox::create`, `sandbox::exec`, `sandbox::list`,
  `sandbox::stop`, `sandbox::upload`, `sandbox::download`.
- The `iii-exec` worker is registered at `engine/src/workers/shell/worker.rs:68` with
  `enabled_by_default=false`.
- Sandbox VMs use a pile of internal-infra crates: `iii-init` (PID 1 inside the VM), `iii-network`
  (smoltcp), `iii-filesystem` (passthrough FS), `iii-shell-proto` / `iii-shell-client` (the exec
  wire protocol), `iii-supervisor` (process supervisor).
- `iii-worker` (the `crates/iii-worker` binary) currently hosts the `iii sandbox` CLI surface (and
  the `iii worker exec` shell-client plumbing).

## What we need to know

1. **Which worker hosts the `sandbox::*` functions?**
   - Is it `iii-exec` (the existing one in `engine/src/workers/shell/`)? If yes, the functions are
     added to that worker.
   - Is it a new worker (e.g., `iii-sandbox`)? If yes, where is it registered, and is `iii-exec`
     deprecated or kept distinct?
   - Or does `iii-worker-manager` (which manages worker lifecycle) host them — given sandboxes are
     ephemeral worker instances?

2. **What's the relationship between `iii-exec` and the sandbox concept?**
   - The shell exec channel (`iii-shell-proto` / `iii-shell-client`) is plumbing for
     `iii worker exec`. Is `iii-exec` a fully separate worker from the sandbox facility, or are they
     the same surface?

3. **Does the sandbox worker need its own ideal-docs callout?**
   - Per the new general rule in `project-rules/workers.md`: broadly-important Worker Docs surfaces
     get callouts in ideal-docs. Sandbox is broadly important for AI coding agents (per user). If
     the worker name is decided, decide whether to add a callout on `using-iii/workers.mdx` near the
     existing exec stub.

## What to do once clarified

- Update [`WORK.workers.md`](./WORK.workers.md) to use the resolved worker name in the per-worker
  checklist.
- Add a worker-specific entry if it's a new worker (currently the workers table lists `iii-exec`; if
  a new `iii-sandbox` exists, add it).
- Update [`WORK.cli.md`](./WORK.cli.md) §4 with the resolved function-namespace for the sandbox
  functions.
- Decide on the sandbox callout placement (if applicable).
