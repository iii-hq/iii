# CLI rules

Rules for the `iii` CLI surface and how it's documented.

The CLI is under active development and its surface is changing frequently. Every check in this
file that validates a command, subcommand, flag, or argument against what we know is
**Severity: warning.** — never error on CLI-surface knowledge, because our knowledge lags the CLI.

## `iii noun verb` naming convention

Commands and subcommands follow the pattern `iii <noun> <verb>` — the noun (what you're acting on)
precedes the verb (what you're doing).

Reviewing a CLI command involves **two independent checks**. Don't conflate them — they have
different fixes and different severities.

1. **Convention check.** **Severity: warning.** Does the command form match `iii <noun> <verb>`?
   If the second word is not a noun (e.g., `iii build worker`, `iii deploy <noun>`) or the order is
   reversed, flag it as a *convention violation*. Real English verbs like `reinstall`, `rebuild`,
   `recheck` satisfy the convention; do not flag them on convention grounds just because they
   start with `re-`. Naming is a guideline rather than a hard requirement; the right form is often
   debatable, so this check surfaces as a warning rather than an error.

2. **Recognized-list check. Severity: warning.** The catalog below is a *partial* list of commands
   we happen to know about — it is **not** a complete inventory of the CLI. You do not know every
   command, subcommand, flag, or argument that exists. So when a command, subcommand, flag, or
   argument doesn't appear in the catalog, assume it's **likely valid** and that the gap is in our
   list, not the docs. At most surface it as a **warning** ("couldn't confirm this against our
   command list — verify it exists"), never an error, and link a replacement only when the catalog
   explicitly documents a removal/rename (see the `iii sandbox` case below). When in doubt, stay
   silent. The convention check (#1) is independent of this and still applies.

3. **"Re-" verbs are still verbs.** `reinstall`, `restart`, `reset`, `rebuild` all satisfy the
   noun-verb convention. Do not flag them on convention grounds.

### Recognized iii commands

A partial list of commands we know about, gathered from `iii <noun> --help`. This is **not**
exhaustive — the CLI has commands, subcommands, and flags not listed here. A noun-verb pair that's
absent is unconfirmed, not wrong; at most warn (see the recognized-list check above), don't error.

**`iii worker`** subcommands:
- `iii worker init`
- `iii worker add`
- `iii worker remove`
- `iii worker reinstall`
- `iii worker update`
- `iii worker clear`
- `iii worker start`
- `iii worker stop`
- `iii worker restart`
- `iii worker list`
- `iii worker sync`
- `iii worker status`
- `iii worker logs`
- `iii worker exec`

`iii worker add` accepts Registry package references, OCI references, and local worker paths.
Local paths use the worker's `iii.worker.yaml`; `iii worker init` scaffolds that public manifest.
Compose remains available for multi-worker project stacks and release descriptor authoring.

**`iii compose`** subcommands:
- `iii compose init`
- `iii compose validate`
- `iii compose descriptor`
- `iii compose descriptor-index`
- `iii compose build`
- `iii compose logs`

Bare `iii compose` serves the Compose control surface and `iii compose --up` starts the selected
stack from `worker-compose.yaml`.

**`iii cloud`** subcommands: `login`, `logout`, `whoami`, `context`, `orgs`, `projects`, `envs`,
`deploy`, `deployments`, `versions`, `vars`, `domains`, `api-keys`, `registry`, `completions`,
`push`.

**Sandbox** is invoked through the trigger mechanism: `iii trigger sandbox::<verb>` (e.g.
`iii trigger sandbox::run`, `iii trigger sandbox::exec`). The earlier CLI forms — top-level
`iii sandbox <verb>` and `iii worker sandbox <verb>` — are outdated. **Severity: warning.** Flag
either form as *outdated* with the replacement `iii trigger sandbox::<verb>`.

**`iii project`** subcommands: `init`, `generate-docker`.

**Recognized flags** (partial, like the rest of this catalog): `--json` (machine-readable output;
valid on many commands), `--config`, `--use-default-config`, `--no-update-check`, `--version`,
`--list-targets`, and `-n` / `--namespace` on `iii trigger`. Never flag a flag as invalid — at
most warn that it couldn't be confirmed.

**Verbless top-level commands** (take args, not a verb): `iii trigger <function-path>`,
`iii console`, `iii update`. The `iii trigger` form is documented separately below.

**Recognized exemption — `iii trigger`:** The syntax
`iii trigger [-n <namespace>|--namespace <namespace>] <function-path> [argA="value" argB=5 ...]`
is the canonical way to invoke any registered function from the CLI (e.g.,
`iii trigger sandbox::run`, `iii trigger state::set`, `iii trigger iii::durable::publish`). The
`function-path` follows the worker-namespaced `noun::verb` scheme.

When you encounter another command that doesn't follow `iii noun verb`, flag it on *convention*
grounds — either the command name should change, or the doc should clarify the noun. When the
command follows the convention but isn't recognized, flag it on *recognized-list* grounds and link
to the replacement if one exists.

## `iii worker` CLI is iii-level tooling

All `iii worker` subcommands (see the recognized list above) plus the `iii.lock` lockfile are part
of iii itself, analogous to `npm`/`cargo`. Catalog, local build, project runtime, and release
descriptor authoring belong to `iii compose`. They are documented in the iii docs, not Worker
Docs.

`using-iii/cli.mdx` should reference `using-iii/workers.mdx` for `iii worker` subcommand details
rather than duplicate them.

## `using-iii/cli.mdx` scope

**Severity: warning.** The CLI page covers:

- Engine flags (`--config`, `--use-default-config`, `--no-update-check`, `--version`). This list
  is partial — other flags (e.g. `--json`) exist and are valid; do not flag a documented flag
  just because it isn't listed here.
- Cross-cutting CLI verbs that aren't tied to a noun (`iii trigger ...` for invoking functions, if
  that survives).
- A pointer to `using-iii/workers.mdx` for `iii worker` subcommands.

It does **not** enumerate every `iii worker` subcommand — those are on the noun's primary page
(`using-iii/workers.mdx`).
