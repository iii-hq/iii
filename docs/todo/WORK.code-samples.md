# Verify all code samples against actual execution before publishing

Every code block in `ideal-docs` (bash, TypeScript, Python, Rust, YAML config, MDX components) must
be exercised against a real iii engine / SDK release before the docs ship. Today many were drafted
from `dx-improves` source inspection and pattern-matching, not from a working installation.

## Why

A doc sample that doesn't run is a debt the reader inherits silently. The current pages that have
the highest concentration of unexecuted snippets:

- `quickstart.mdx` — `iii project init --template quickstart`, `iii trigger math::add a=2 b=3`, the
  Python and TypeScript worker source, the `iii worker add iii-state` / `iii-http` flow, the
  `curl -X POST ...` HTTP example.
- `using-iii/workers.mdx` — `iii worker add <worker-name>`, `iii worker start/stop/restart`, the
  TypeScript / Python / Rust `registerWorker` tabs in the Connected workers section.
- `using-iii/functions.mdx` — `worker.registerFunction` and `worker.trigger` tabs, the
  `TriggerAction.Void` and `TriggerAction.Enqueue` API names.
- `using-iii/triggers.mdx` — `worker.registerTrigger` tabs, `worker.unregisterTrigger`.
- `using-iii/engine.mdx` — `iii --config config.yaml`, `${VAR:default}` env expansion examples, the
  `--use-default-config` flag.
- `using-iii/registry.mdx` — `iii worker add <worker-name>@1.2.0`, `iii worker remove`,
  `iii worker update`.
- `install.mdx` — the `curl | sh` installer, `iii --version`, the VS Code / Cursor / Windsurf /
  VSCodium `--install-extension` commands (still under the section-3 TODO wrap).

## What to do

For each page above:

1. Stand up a clean iii engine + SDK install matching the version the doc targets.
2. Run every CLI command in order. Capture exact output where the doc shows expected output.
3. Compile / run the SDK snippets in their respective tabs. The same handler should work in TS,
   Python, and Rust.
4. Reconcile drift: rename APIs that don't exist, drop tabs whose SDK doesn't ship the surface yet,
   correct any sample output (`pid: 12345` etc.) that's a placeholder.

## Scope cap

Verification is a pre-publish gate, not an ongoing churn. Once each page passes, mark it in the repo
(a `verified-at: <date> against iii <version>` frontmatter field would be ideal; for now, a
`{/* verified: <date> against iii <version> */}` MDX comment on each page is enough).

## Out of scope

Code samples in `expanding-iii/*` and `sdk-reference/*` are largely still placeholders. Add them to
this list only when those pages are flesh-out passes worth verifying.
