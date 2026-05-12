# CLI rules

Rules for the `iii` CLI surface and how it's documented.

## `iii noun verb` naming convention

Commands and subcommands follow the pattern `iii <noun> <verb>` — the noun (what you're acting on)
precedes the verb (what you're doing).

Examples that conform:

- `iii project init`
- `iii worker init`
- `iii worker add`
- `iii worker remove`
- `iii worker list`
- `iii worker start`
- `iii worker stop`
- `iii worker exec`
- `iii worker update`
- `iii worker verify`

**Recognized exemption — `iii trigger`:** The syntax
`iii trigger <function-path> [argA="value" argB=5 ...]` is the canonical way to invoke any
registered function from the CLI (e.g., `iii trigger sandbox::run`, `iii trigger state::set`,
`iii trigger iii::durable::publish`). The `function-path` follows the worker-namespaced `noun::verb`
scheme.

When you encounter another command that doesn't follow `iii noun verb`, flag it — either the command
name should change, or the doc should clarify the noun.

## `iii worker` CLI is iii-level tooling

All `iii worker` subcommands — `add/remove/list/start/stop/exec/update/verify` — plus the `iii.lock`
lockfile and worker image build/publish flow are part of iii itself, analogous to `npm`/`cargo`.
They are documented in the iii docs (primarily `using-iii/workers.mdx`), not Worker Docs.

`using-iii/cli.mdx` should reference `using-iii/workers.mdx` for `iii worker` subcommand details
rather than duplicate them.

## `using-iii/cli.mdx` scope

The CLI page covers:

- Engine flags (`--config`, `--use-default-config`, `--version`).
- Cross-cutting CLI verbs that aren't tied to a noun (`iii trigger ...` for invoking functions, if
  that survives).
- A pointer to `using-iii/workers.mdx` for `iii worker` subcommands.

It does **not** enumerate every `iii worker` subcommand — those are on the noun's primary page
(`using-iii/workers.mdx`).
