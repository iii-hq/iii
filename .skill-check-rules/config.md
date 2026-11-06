# Configuration rules

Rules for configuration file naming and conventions.

## Config file names

`config.yaml` is the canonical filename for runtime configuration. It appears in two distinct contexts that share a name but never share state:

- **Engine config** — `config.yaml`. The engine reads it from the cwd (the directory `iii` was started in) or from an explicit path via `iii --config /path/to/config.yaml`. The engine does not walk parent directories looking for it. Carries engine-wide settings (workers list, ports, telemetry).
- **Worker runtime config** — `<worker>/config.yaml`. Each worker keeps its own runtime config alongside `iii.worker.yaml`. Rendered verbatim into the worker README under `## Configuration`.

The two never share state or schema; their location and consumer are different.

The worker manifest is `iii.worker.yaml` — separate from the worker's runtime `config.yaml`. The manifest carries identity (`name`, `language`, `deploy`, `bin`); the runtime config carries values the worker reads at boot.

A Compose project is declared in `worker-compose.yaml` (the compose file `iii compose --up` and
`compose::up` read by default) and pinned by `worker-compose.lock`. The compose file lists the
project's workers and its engine; it is a separate file from the engine's `config.yaml` and from a
worker's `iii.worker.yaml`. A doc that names `worker-compose.yaml` as the compose file is correct.

Runtime configuration entries managed by the `configuration` worker live as `<id>.yaml` files in
its `fs` adapter directory (`./config` by default).

When source content references `iii-config.yaml`, normalize to `config.yaml` in any stub. Note the rename in the decisions log if the source page is being absorbed.

## Config reference is auto-generated (planned)

The configuration reference (the per-field schema for `config.yaml`) is intended to be auto-generated from a commented YAML source file colocated with the engine, then transcluded into `using-iii/engine.mdx`. A pre-Mintlify implementation (parser + React component, commit `0f925fd2` in iii-mono) was dropped during the Mintlify migration.

Until restored:
- Don't hand-author per-field schema content in the iii docs.
- The "Engine configuration" stub on `using-iii/engine.mdx` is a placeholder for the eventual generated content.

