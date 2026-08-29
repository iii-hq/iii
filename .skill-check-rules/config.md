# Configuration rules

Rules for configuration file naming and conventions.

## Config file names

`config.yaml` is the canonical filename for direct engine runtime configuration. The root
`worker-compose.yaml` is the canonical source for worker catalogs, builds, package runtime,
release metadata, validation, and project stacks. They have different consumers and never share
state:

- **Engine config** — `config.yaml`. The engine reads it from the cwd (the directory `iii` was started in) or from an explicit path via `iii --config /path/to/config.yaml`. The engine does not walk parent directories looking for it. Carries engine-wide settings (workers list, ports, telemetry).
- **Worker project config** — `worker-compose.yaml`. A single root document owns `workers` and
  `stacks`; per-worker non-secret runtime defaults live under `workers.<key>.runtime`, and
  per-container overrides live under `stacks.<stack>.containers.<key>.config`.

The two never share state or schema; their location and consumer are different.

There is no per-worker manifest and no compatibility path for one. A worker's key under `workers`
is its identity, its package manifest supplies its version, and the descriptor compiler combines
those inputs into `release-descriptor.json`. Do not recommend adding, reading, or generating a
per-worker manifest.

When source content references `iii-config.yaml`, normalize to `config.yaml` in any stub. Note the rename in the decisions log if the source page is being absorbed.

## Config reference is auto-generated (planned)

The configuration reference (the per-field schema for `config.yaml`) is intended to be auto-generated from a commented YAML source file colocated with the engine, then transcluded into `using-iii/engine.mdx`. A pre-Mintlify implementation (parser + React component, commit `0f925fd2` in iii-mono) was dropped during the Mintlify migration.

Until restored:
- Don't hand-author per-field schema content in the iii docs.
- The "Engine configuration" stub on `using-iii/engine.mdx` is a placeholder for the eventual generated content.
