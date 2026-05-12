# WORK — Console

Remaining work on the iii Console (the standalone web UI). Sourced from
[`PROGRESS.md`](../PROGRESS.md) and [`PROGRESS-repo-checks.md`](../PROGRESS-repo-checks.md).

Repo: `iii-temp/console/` (`console-frontend` React + `console-rust` Axum server). Docs page:
`using-iii/console.mdx`. Project rule: [`project-rules/console.md`](../project-rules/console.md).

## Survey result: aligned

The audit found the console code matches the rules and the stub page exactly. **No docs-side work is
open.** This file exists for future reference / regression checks.

Confirmed alignment:

- [x] Sidebar order (Workers → Functions → Triggers → States → Streams → Queues → Traces → Logs →
      Config; plus Flow when `enableFlow=true`).
- [x] Default ports (3111 engine HTTP / 3112 engine WS / 3113 console UI / 49134 SDK bridge WS).
- [x] Flow page feature flag (`--enable-flow` / `III_ENABLE_FLOW`).
- [x] Console = standalone surface (not a worker; documentation in iii docs, not Worker Docs).
- [x] UI page → worker mapping is one-to-one with no missing surfaces.
- [x] Launch instructions correct (`iii console`, internal binary `iii-console` not surfaced to
      users).

## Watch items (no current work)

These are not actively open but warrant a recheck if certain other decisions land:

- [ ] If the iii-engine-functions worker is renamed (see `PROGRESS-repo-checks.md` hanging piece on
      naming review), the Console "Workers", "Functions", and "Triggers" pages may surface the
      rename in their headers / tooltips. Audit afterward.
- [ ] If the `engine::` prefix on observability functions changes (e.g., `engine::log::*` →
      `obs::log::*`), the Console "Traces" and "Logs" pages call those functions by name; audit
      afterward.
- [ ] If introspection consolidates into a dedicated worker (engineering review item), the Console
      surfaces will rebind to that worker. Audit afterward.
- [ ] If `iii-http-functions` renames (see `PROGRESS-repo-checks.md` hanging piece), the Console
      "Config" page enumerates workers — the new name will appear there.

## When the console gains new functionality

If the GUI gains new functionality (per user comment about introspection: "if it needs to be
improved gui can add to it"):

- [ ] Add a corresponding stub on `using-iii/console.mdx`. Match the existing one-line stub style.
- [ ] Confirm any new UI page maps to a known worker (per `console.md` rule that each page renders
      data from a specific worker).
- [ ] If the new page surfaces a worker, no Worker Docs change needed; the Console UI doc is
      console-owned.

## Authoring (when stubs become prose)

- [ ] Apply `project-rules/voice.md` (declarative, confident).
- [ ] Cross-link to iii-engine-functions and iii-observability Worker Docs from the introspection
      callout already at the top of the page.
- [ ] Keep the `console.md` rule that each Console UI page is documented from the console's
      perspective ("how do I use the States page"), not from the worker's perspective ("how does
      iii-state work" — link out for that).

## Dead code detection

See [`WORK.dead-code.md`](./WORK.dead-code.md) for the cross-cutting initiative. Console-specific
items:

- [ ] **`console-frontend`:** add `knip` with a baseline ignore list; drive to zero. Add
      `@typescript-eslint/no-unused-vars` to ESLint.
- [ ] **`console-rust`:** enable `#![warn(dead_code, unused_imports, unused_variables)]` at the
      crate root; add `cargo machete` to CI.
- [ ] After any worker rename (see "Watch items"), audit the frontend for stale worker-name string
      constants.

## Notes / dependencies

- No active work blocked on the console.
- All "watch items" are downstream of decisions tracked in [`WORK.engine.md`](./WORK.engine.md) and
  [`WORK.workers.md`](./WORK.workers.md).
