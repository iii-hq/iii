# Configuration rules

Rules for configuration file naming and conventions.

## Config file names

- **Engine config:** `config.yaml` (not `iii-config.yaml`).
- **Worker-level config:** `iii.worker.yaml`.

When source content references `iii-config.yaml`, normalize to `config.yaml` in any stub. Note the
rename in the decisions log if the source page is being absorbed.

## Config reference is auto-generated (planned)

The configuration reference (the per-field schema for `config.yaml`) is intended to be
auto-generated from a commented YAML source file colocated with the engine, then transcluded into
`using-iii/engine.mdx`. A pre-Mintlify implementation (parser + React component, commit `0f925fd2`
in iii-mono) was dropped during the Mintlify migration.

Until restored:

- Don't hand-author per-field schema content in the iii docs.
- The "Engine configuration" stub on `using-iii/engine.mdx` is a placeholder for the eventual
  generated content.

## Adapter config is deprecated

Anywhere the source mentions `adapter:` blocks (Redis, RabbitMQ, KV, etc.), the content should be
removed from the iii docs. See [`general.md`](./general.md) for the adapter-deprecation rule.
