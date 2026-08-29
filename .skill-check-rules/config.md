# Configuration rules

Rules for configuration file naming and conventions.

## Config file names

`config.yaml` is the canonical filename for direct engine runtime configuration. The root
`worker-compose.yaml` is the canonical source for release catalogs, descriptor builds, package
runtime, release metadata, validation, and Compose project stacks. Public per-worker manifests
remain a separate compatibility and development contract:

- **Engine config** — `config.yaml`. The engine reads it from the cwd (the directory `iii` was started in) or from an explicit path via `iii --config /path/to/config.yaml`. The engine does not walk parent directories looking for it. Carries engine-wide settings (workers list, ports, telemetry).
- **Worker project config** — `worker-compose.yaml`. A single root document owns `workers` and
  `stacks`; per-worker non-secret runtime defaults live under `workers.<key>.runtime`, and
  per-container overrides live under `stacks.<stack>.containers.<key>.config`.
- **Public worker manifest** — `iii.worker.yaml`. A worker-local document used by `iii worker`,
  local/OCI/Registry package installation, templates, and manifest-based development tooling.

The three do not share a schema; their location and consumer are different.

Within a release catalog, a worker's key under `workers` is its canonical release identity, its
package manifest supplies its version, and the descriptor compiler combines those inputs into
`release-descriptor.json`. Release tooling must not read or fall back to `iii.worker.yaml`. This
restriction does not apply to the public `iii worker` surface.

When source content references `iii-config.yaml`, normalize to `config.yaml` in any stub. Note the rename in the decisions log if the source page is being absorbed.

## Config reference is auto-generated (planned)

The configuration reference (the per-field schema for `config.yaml`) is intended to be auto-generated from a commented YAML source file colocated with the engine, then transcluded into `using-iii/engine.mdx`. A pre-Mintlify implementation (parser + React component, commit `0f925fd2` in iii-mono) was dropped during the Mintlify migration.

Until restored:
- Don't hand-author per-field schema content in the iii docs.
- The "Engine configuration" stub on `using-iii/engine.mdx` is a placeholder for the eventual generated content.
