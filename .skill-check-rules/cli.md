# CLI rules

Rules for the `iii` CLI surface and how it's documented.

The CLI is under active development and its surface is changing frequently. Every check in this
file that validates a command, subcommand, flag, or argument against what we know is
**Severity: warning.** — never error on CLI-surface knowledge, because our knowledge lags the CLI.
Emit every finding from this file with the `:warning` token, never as an error.

The generated CLI reference (`cli-reference/index.mdx`, produced from the binary's clap
definitions) is the authoritative list of commands and flags. When an artifact matches it, the
command is valid.

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
   debatable, so this check surfaces as a warning rather than an error. The verbless top-level
   commands listed below are exempt: do not flag them on convention grounds.

2. **Recognized-list check. Severity: warning.** The catalog below is a *partial* list of commands
   we happen to know about — it is **not** a complete inventory of the CLI. You do not know every
   command, subcommand, flag, or argument that exists. So when a command, subcommand, flag, or
   argument doesn't appear in the catalog, assume it's **likely valid** and that the gap is in our
   list, not the docs. At most surface it as a **warning** ("couldn't confirm this against our
   command list — verify it exists"), never an error, and link a replacement only when the catalog
   explicitly documents a removal/rename (see the removed commands below). When in doubt, stay
   silent. The convention check (#1) is independent of this and still applies.

3. **"Re-" verbs are still verbs.** `reinstall`, `restart`, `reset`, `rebuild` all satisfy the
   noun-verb convention. Do not flag them on convention grounds.

### Recognized iii commands

A partial list of commands we know about, gathered from `iii --help` and `iii <noun> --help`. A
noun-verb pair that's absent is unconfirmed, not wrong; at most warn (see the recognized-list check
above), don't error.

**`iii`** with no subcommand starts the engine from `./config.yaml` or `--config <path>`.

**Verbless top-level commands** (take args or flags, not a verb; exempt from the convention check):

- `iii call <function-path> [key=value ...]` — call a registered function on a running engine.
  `iii trigger` and `iii t` are aliases of `iii call`; both forms are valid. Flags include
  `--json <JSON>`, `--engine <URL>`, `--timeout-ms <MS>`, and `-n, --namespace <NS>`. Run
  `iii call <function-path> --help` to see a function's arguments.
- `iii compose` — run the Compose daemon. `--up` also brings up a project from its compose file
  (`-f, --file <PATH>`, default `./worker-compose.yaml`) together with its declared engine, unless
  `--engine <URL>` selects an existing one. Other flags: `-n, --namespace <NS>`, `--frozen`.
  Subcommands: `iii compose build` (download the compose file's registry packages) and
  `iii compose logs` (read worker stdout and stderr from a running Compose daemon).
- `iii ps` — list iii processes on this machine (engines, compose daemons, workers). `--json`.
- `iii kill` — stop iii processes: `--port <PORT>`, `--compose <PATH>`, `--orphans`, `--force`.
- `iii update [COMMAND]` — update iii and its managed binaries; `--list-targets`.

`iii compose --up` is the normal way to run a project. Do not flag `--up` as a convention violation:
it is a flag of the verbless `iii compose` command.

**`iii project`** subcommands: `init`, `generate-docker`.

**`iii cloud`** subcommands: `login`, `logout`, `whoami`, `context`, `orgs`, `projects`, `envs`,
`deploy`, `deployments`, `versions`, `vars`, `domains`, `api-keys`, `registry`, `completions`,
`push`.

**Engine flags**: `-c, --config <CONFIG>` (default `config.yaml`), `-v, --version`,
`--no-update-check`. Never flag a flag as invalid — at most warn that it couldn't be confirmed.

### Functions called from the CLI

`iii call <function-path>` invokes any registered function, for example `iii call state::get`,
`iii call engine::functions::list`, `iii call compose::add worker=ade`, or
`iii call -n dev compose::status`. The `function-path` follows the worker-namespaced `noun::verb`
scheme. Project workers are added and managed through the Compose worker's `compose::*` functions
(`compose::add`, `compose::remove`, `compose::restart`, `compose::status`, and the rest).

**Sandbox** is invoked the same way: `iii call sandbox::<verb>` (e.g. `iii call sandbox::run`,
`iii call sandbox::exec`).

### Removed commands

Flag these as *outdated* (**Severity: warning.**) when an artifact presents them as current, with
the replacement shown. An artifact that says they were removed, and names the replacement, is
correct: do not flag it.

- `iii worker` and all its subcommands (`init`, `add`, `remove`, `reinstall`, `update`, `clear`,
  `start`, `stop`, `restart`, `list`, `sync`, `verify`, `status`, `logs`, `exec`), together with the
  `worker::*` functions, were removed in 0.23. Replacement: `iii compose` and the `compose::*`
  functions, called with `iii call compose::<verb>`.
- `iii console` prints a notice that points to the console in ADE and exits; the console is the ADE
  worker (`ade` in the registry), added with `iii call compose::add worker=ade`.
- Top-level `iii sandbox <verb>` and `iii worker sandbox <verb>`. Replacement:
  `iii call sandbox::<verb>`.

When you encounter another command that doesn't follow `iii noun verb` and is not exempt above,
flag it on *convention* grounds — either the command name should change, or the doc should clarify
the noun. When the command follows the convention but isn't recognized, flag it on
*recognized-list* grounds and link to the replacement if one exists.

## Project tooling is iii-level

`iii compose`, its compose file (`worker-compose.yaml`), its lockfile (`worker-compose.lock`), and
the `compose::*` functions are part of iii itself, analogous to `npm`/`cargo`. They are documented
in the iii docs (primarily `using-iii/compose.mdx` and `using-iii/workers.mdx`), not Worker Docs.

## `using-iii/cli.mdx` scope

**Severity: warning.** The CLI page covers:

- Engine flags (`--config`, `--version`, `--no-update-check`). This list is partial — other flags
  (e.g. `--json`) exist and are valid; do not flag a documented flag just because it isn't listed
  here.
- The top-level commands (`iii compose`, `iii call`, `iii project`, `iii cloud`, `iii update`,
  `iii ps`, `iii kill`) and the removed ones with their replacements.
- Pointers to the generated CLI reference and to `using-iii/workers.mdx` for managing project
  workers.

It does **not** enumerate every flag of every command — those are in the generated CLI reference.
