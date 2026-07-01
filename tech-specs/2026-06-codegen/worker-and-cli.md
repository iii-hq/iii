# Worker & CLI

`codegen` is one Rust binary with two front doors over a shared core pipeline
(*select → discover → map → emit → write*, [README](README.md#architecture)): a
`clap` CLI for local/CI use, and `codegen::*` functions for use from other
workers and agents. Both connect to the engine as a transient worker, run the
pipeline, and report identically.

Rust is the implementation language because codegen must run as a **single
self-contained binary inside any project** — a TypeScript, Python, *or* Go repo —
with no language runtime to install, exactly the `deploy: binary` model
[`coder`](../../coder) uses. (This is unrelated to the languages it *emits*,
which are always TS/JS/Rust/Python.)

## File layout

Mirrors the binary-worker SOP and `coder`:

```
workers/codegen/
  Cargo.toml                 # [[bin]] name = "codegen"
  iii.worker.yaml            # deploy: binary, multi-target
  src/
    main.rs                  # clap CLI; dispatches subcommands
    lib.rs                   # the pipeline, re-used by CLI and worker
    config.rs                # codegen.yml parse + validate (serde_yaml)
    catalog.rs               # discovery: engine::* calls → in-memory catalog
    select.rs                # glob selection (globset)
    schema/                  # JSON Schema → IR (intermediate representation)
      mod.rs                 #   parse, $defs collection, $ref resolution
    emit/
      mod.rs                 #   shared skeleton + naming
      typescript.rs
      javascript.rs
      rust.rs
      python.rs
    worker.rs                # register codegen::generate / ::preview / ::languages
    manifest.rs              # build_manifest() for --manifest
  tests/
    golden/                  # fixture catalogs + expected outputs (see Testing)
```

## CLI surface

`clap` derive, matching `coder`'s `--url` / `--manifest` conventions
(`workers/coder/src/main.rs:14-49`):

```
codegen generate  --config <path> [--url <ws>] [--only <path>]... [--check] [--watch]
codegen preview   --config <path> [--url <ws>] --output <path>     # prints code to stdout, writes nothing
codegen languages                                                  # prints supported languages + status
codegen --manifest                                                 # prints the worker manifest JSON, exits
```

| Flag | Default | Meaning |
|---|---|---|
| `--config` | `./codegen.yml` | path to the config file |
| `--url` | `$III_URL` then `ws://127.0.0.1:49134` | engine address |
| `--only` | (all) | restrict to specific output path(s); repeatable |
| `--check` | off | compute outputs, write nothing, exit 1 if any would change ([emitters § Determinism](emitters.md#determinism--idempotency)) |
| `--watch` | off | re-run on `codegen.yml` change **and** on `engine::functions::available` (the engine's catalog-change trigger) |

`generate` exits `0` on success (all written/unchanged), `1` under `--check` when
something would change, and `2` on a config or connection error. The human output
is the same per-output report the worker function returns, rendered as a table.

## Exposed functions

Three typed functions (typed handlers only, per
[Conventions](README.md#conventions)).

### `codegen::generate`

Runs the pipeline and writes files (relative to `cwd`, or `base_dir` if given).

```jsonc
// request
{
  "config_path": "string?",     // path to codegen.yml — OR —
  "config": { /* inline GenerationConfig, same schema as the file */ },
  "base_dir": "string?",        // resolve output paths against this (default: cwd)
  "only": ["string"],           // optional subset of output paths
  "check": false                // dry-run; write nothing, report would-change
}
// exactly one of config_path / config is required

// response
{
  "outputs": [
    { "path": "src/types/codegen/harness.ts",
      "language": "typescript",
      "functions": 7, "triggers": 2, "types": 11, "bytes": 4210,
      "status": "written" }       // written | unchanged | would-change
  ],
  "warnings": ["functions glob 'harness::*' matched nothing — is harness running?"]
}
```

The inline `config` is validated against the same JSON Schema the file is
([configuration § config schema](configuration.md#the-configs-own-json-schema)),
which is published as this function's `request_format`.

### `codegen::preview`

Identical request to `codegen::generate` but **never writes** — returns the
generated source text, for agents and tooling that want to show or diff output
before committing.

```jsonc
// response
{ "outputs": [ { "path": "…", "language": "…", "code": "<generated source>" } ],
  "warnings": [ "…" ] }
```

### `codegen::languages`

Capability probe — what this build can emit.

```jsonc
// request: {}
// response
{ "languages": [
  { "id": "typescript", "status": "stable", "modes": ["types","functions","triggers"] },
  { "id": "javascript", "status": "stable", "modes": ["types","functions","triggers"] },
  { "id": "rust",       "status": "stable", "modes": ["types","functions","triggers"] },
  { "id": "python",     "status": "stable", "modes": ["types","functions","triggers"] },
  { "id": "go",         "status": "planned", "modes": [] }
] }
```

## `iii.worker.yaml`

Same shape as `coder`'s (`workers/coder/iii.worker.yaml:1-23`):

```yaml
iii: v1
name: codegen
language: rust
deploy: binary
manifest: Cargo.toml
bin: codegen
description: Generates typed client code (types, function wrappers, trigger handlers) in TS/JS/Rust/Python from the engine's live function catalog.

targets:
  - x86_64-apple-darwin
  - aarch64-apple-darwin
  - x86_64-unknown-linux-gnu
  - x86_64-unknown-linux-musl
  - aarch64-unknown-linux-gnu

runtime:
  kind: rust

scripts:
  install: cargo build
  start: cargo run
```

## Dependencies

| Crate | Purpose |
|---|---|
| `iii-sdk` | `register_worker`, `IIIClient`, `TriggerRequest`, `register_function` (worker mode) |
| `clap` (derive, env) | CLI + `--url`/`$III_URL` binding |
| `serde`, `serde_json` | the catalog/schema `Value`s and the worker function I/O |
| `serde_yaml` | parse `codegen.yml` |
| `schemars` | derive the JSON Schema for codegen's *own* `codegen::*` function I/O |
| `globset` | selection globs ([configuration § Selection](configuration.md#selection-semantics)) |
| `convert_case` | identifier casing in naming derivation |
| `tokio` | async runtime (the SDK is async) |
| `notify` | `--watch` (optional, feature-gated) |

The emitters are **hand-written string builders**, not a templating engine —
output is structured and must be byte-deterministic and pre-formatted, so direct
control beats a template DSL.

## Testing

Codegen is exactly the kind of tool whose bugs are silent (wrong type, off-by-one
optionality) until a consumer miscompiles, so testing is two-layered:

1. **Golden tests** (`tests/golden/`): a set of fixture catalogs — JSON files
   that are literal `FunctionDetail` / `TriggerTypeDetail` responses captured
   from real workers (`harness`, `email`, plus hand-crafted edge cases: `$defs`
   recursion, `oneOf`, nullable, untyped `Value`, path-like ids) — each paired
   with the **expected generated file** per language. The mapper/emitter run
   offline against the fixtures (no engine needed) and the output is
   byte-compared. New behavior = new fixture; this is also the regression net for
   [determinism](emitters.md#determinism--idempotency).
2. **Downstream compile checks** (CI): the generated TS/Rust/Python from the
   golden fixtures is fed to `tsc --noEmit`, `cargo check`, and `pyright` (with
   `pydantic`) respectively. This proves the emitted code is not just
   string-stable but actually *valid and type-correct* in each target toolchain —
   the property that matters most.

A small **live integration test** boots the engine with `todo-worker`, runs
`codegen generate` against it, and asserts the wrappers call the right
`function_id`s — covering the discovery/connection path the golden tests skip.

## Boundaries / non-goals

- **No Go emitter in v1.** `language: go` is reserved and rejected with
  `E_LANG_UNSUPPORTED`; `codegen::languages` reports it `planned`. (Per the
  brief.)
- **Client surface only.** Codegen emits typed *callers*, *types*, and typed
  *trigger-subscription* helpers. It does **not** scaffold worker
  implementations, function bodies, or `iii.worker.yaml` — schemas and behavior
  are owned by the workers that register them.
- **Not a schema authoring tool.** Codegen never invents or edits schemas; it is
  a pure projection of `engine::*::info` output.
- **Live catalog only (v1).** Generation requires the target workers to be
  connected ([discovery § live](discovery-and-types.md#the-catalog-is-live)).
  Generating from a checked-in catalog snapshot — so CI need not boot every
  worker — is the main planned v2 addition (`codegen generate --from catalog.json`).
- **No custom templates / plugins (v1).** The `graphql-codegen` plugin model is a
  possible future; v1 ships the four built-in emitters only.
- **Read-mostly, scoped writes.** The only filesystem mutation is writing the
  declared output paths; codegen never deletes files and never writes outside the
  resolved output set.
