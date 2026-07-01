# Configuration — `codegen.yml`

This is the developer-facing contract: a `graphql-codegen`-style file that maps
**output file paths to generation specs**. Each top-level key is a path to write;
its value declares the language, what to generate, which slice of the catalog to
cover, and how the generated code reaches the iii instance at runtime. One file
can describe many outputs in many languages — they are generated independently
and in declaration order.

## Shape

The root is a map of `<output-path>: GenerationSpec`. There is **no global
block** in the file (the engine address comes from `--url` / `$III_URL`, never
the config — see [README § Conventions](README.md#conventions)), so codegen
consumes the brief's file as written (the near-duplicate `harness.py` entry is
elided here for brevity — it is `harness.rs` with `language: python`):

```yaml
src/types/codegen/harness.ts:
  language: typescript
  mode: ["types", "functions", "triggers"]
  workers:
    - "harness"
  functions:
    - "harness::*"
  triggers:
    - "harness"
  iii_instance:
    type: import
    path: "@/services/iii"
    name: iii

src/types/codegen/harness.rs:
  language: rust
  mode: ["types", "functions", "triggers"]
  workers: ["harness"]
  functions: ["harness::*"]
  triggers: ["harness"]
  iii_instance:
    type: argument

src/types/codegen/iii_workers.rs:
  language: rust
  mode: ["types", "functions", "triggers"]
  workers: ["*"]
  functions: ["*"]
  triggers: ["*"]
  iii_instance:
    type: argument
```

Output paths are resolved relative to the **config file's directory**. A parent
directory is created if missing.

## `GenerationSpec` fields

| Field | Type | Required | Description |
|---|---|---|---|
| `language` | enum | yes | `typescript` · `javascript` · `rust` · `python`. `go` is **reserved but rejected** (E_LANG_UNSUPPORTED) — see [Boundaries](worker-and-cli.md#boundaries--non-goals). |
| `mode` | string[] | yes | Non-empty subset of `["types", "functions", "triggers"]`. Order is irrelevant; duplicates are deduped. |
| `workers` | glob[] | no¹ | Worker-name globs. Selects every function/trigger belonging to a matched worker. |
| `functions` | glob[] | no¹ | `function_id` globs (e.g. `harness::*`, `*`). Selects matching functions. |
| `triggers` | glob[] | no¹ | Trigger selectors — match a trigger-type `id` **or** its owning `worker_name`. |
| `iii_instance` | object | when `functions`/`triggers` emit callable code² | How wrappers obtain the iii client: `import` or `argument`. See [iii_instance](#iii_instance). |

¹ At least one of `workers` / `functions` / `triggers` must be present, or the
output selects nothing and codegen errors with `E_EMPTY_SELECTION` (a no-op
output is almost always a config mistake).

² `iii_instance` is required whenever `mode` contains `functions` or `triggers`
(those modes emit code that calls / registers against the client). It is ignored
— and may be omitted — for a `mode: ["types"]` output, which emits only type
declarations with no runtime dependency.

### `language` → file extension

`language` is **authoritative**; the path extension is just the filename. The
two should agree, but codegen does not infer language from the extension.

| `language` | Emits | Types as |
|---|---|---|
| `typescript` | `.ts` | `export interface` / `export type` |
| `javascript` | `.js` | JSDoc `@typedef` blocks (JS has no static types) |
| `rust` | `.rs` | `#[derive(Serialize, Deserialize)] pub struct` |
| `python` | `.py` | Pydantic v2 `class … (BaseModel)` |

### `mode`

| Mode | Emits | Requires `iii_instance` |
|---|---|---|
| `types` | Type declarations for every selected function's input/output and every selected trigger's config/payload/return. The other modes imply this — `types` makes a **types-only** output. | no |
| `functions` | A typed wrapper per selected function, grouped into a per-worker namespace object (`harness::send` → `harness.send(input)`), each lowering to `iii.trigger({ function_id, payload })`. Pulls in the input/output types. | yes |
| `triggers` | A typed **registration helper** per selected trigger type, nested on the same namespace (`email::new-mail` → `email.onNewMail(config, handler)`), plus its config/payload/return types. | yes |

The shape each mode emits per language is specified in [emitters.md](emitters.md).

## Selection semantics

Selection runs **once per output**, against the live catalog
([discovery-and-types.md](discovery-and-types.md#discovery-the-input-contract)).
The three lists are **independent include filters; the result is their union.**
There is no exclude syntax in v1.

**Functions selected for an output** = every discovered function whose
`function_id`

- matches any glob in `functions`, **or**
- whose `worker_name` matches any glob in `workers`.

**Trigger types selected for an output** = every discovered trigger type whose
`id` **or** `worker_name` matches any glob in `triggers`. (Trigger types are
matched on both because some ids are worker-scoped like `email::new-mail` and
some are global like `http` / `cron`.)

> **Union, by design.** In the brief's `harness.ts`, `workers: ["harness"]` and
> `functions: ["harness::*"]` select the same set — the redundancy is harmless.
> To scope an output to one worker, set the lists consistently (as the example
> does). `["*"]` everywhere means "the whole catalog" (the `iii_workers.rs`
> output). An **empty/absent** list contributes **nothing** — it never means
> "all"; only `["*"]` means all.

### Glob grammar

Globs are matched with [`globset`](https://docs.rs/globset) over the full
`function_id` / worker name / trigger id string, treating `::` as ordinary
characters:

| Pattern | Matches |
|---|---|
| `*` | everything (any run of characters, including `::`) |
| `harness::*` | every function id beginning `harness::` |
| `harness` | the worker named exactly `harness` (in `workers`/`triggers`) |
| `harness::send` | exactly that function id |
| `email::accounts::*` | every id under the `email::accounts::` sub-namespace |

Worker name for a function is its first `::` segment, exactly as the engine
derives it (`engine_fn/mod.rs:406-408` — `s.split("::").next()`).

### Internal & engine functions

`engine::functions::list` hides handlers flagged `metadata.internal == true`
unless `include_internal: true` is passed (`engine_fn/mod.rs:170-181`). Codegen
calls it **without** that flag, so engine-internal handlers never leak into
generated output. The `engine::*` discovery functions themselves are reachable
only if a `functions`/`workers` glob explicitly matches `engine` (e.g.
`functions: ["engine::*"]`) — `["*"]` does match them, which is intentional:
generating a typed client for the engine's own API is a valid use.

## `iii_instance`

Controls how a generated wrapper gets the client it calls `trigger` on. Two
modes, mirroring the brief:

```yaml
# import: a module-level singleton is imported (TS / JS / Python only)
iii_instance:
  type: import
  path: "@/services/iii"   # import source / module path
  name: iii                # imported symbol; default "iii"

# argument: every wrapper takes the client as its first parameter (all languages)
iii_instance:
  type: argument
```

| `type` | `path` | `name` | Lowering | Valid for |
|---|---|---|---|---|
| `import` | required | default `iii` | Emits one import; wrappers reference the imported symbol | `typescript`, `javascript`, `python` |
| `argument` | — | default `iii` | Every wrapper's first parameter is the client (`iii: &IIIClient` / `iii: IIIClient`) | all languages |

> **Rust + `import` is rejected** (`E_RUST_IMPORT`). Rust has no ambient
> module-level singleton convention for the client, so a Rust output must pass
> the handle explicitly. This is why the brief's two Rust outputs both use
> `type: argument`.

The exact emitted import line and wrapper signatures for each combination are in
[emitters.md § iii_instance lowering](emitters.md#iii_instance-lowering).

## The config's own JSON Schema

Codegen validates `codegen.yml` against this schema before doing anything (and
publishes it as the `request_format` of `codegen::generate`, so the inline-config
path is validated identically — see
[worker-and-cli.md](worker-and-cli.md#codegengenerate)):

```jsonc
{
  "type": "object",
  "description": "Map of output path -> GenerationSpec",
  "minProperties": 1,
  "additionalProperties": {
    "type": "object",
    "required": ["language", "mode"],
    "properties": {
      "language": { "enum": ["typescript", "javascript", "rust", "python"] },
      "mode": {
        "type": "array", "minItems": 1, "uniqueItems": true,
        "items": { "enum": ["types", "functions", "triggers"] }
      },
      "workers":   { "type": "array", "items": { "type": "string" } },
      "functions": { "type": "array", "items": { "type": "string" } },
      "triggers":  { "type": "array", "items": { "type": "string" } },
      "iii_instance": {
        "type": "object",
        "required": ["type"],
        "properties": {
          "type": { "enum": ["import", "argument"] },
          "path": { "type": "string" },
          "name": { "type": "string", "default": "iii" }
        },
        "allOf": [{
          "if":   { "properties": { "type": { "const": "import" } } },
          "then": { "required": ["path"] }
        }]
      }
    }
  }
}
```

Cross-field rules the schema can't express (checked in code, each a named error
returned in the report's `warnings` or as a hard failure):

| Rule | Error |
|---|---|
| `iii_instance.type == import` with `language == rust` | `E_RUST_IMPORT` (hard) |
| `mode` contains `functions`/`triggers` but no `iii_instance` | `E_MISSING_INSTANCE` (hard) |
| `workers`, `functions`, `triggers` all empty/absent | `E_EMPTY_SELECTION` (hard) |
| A `functions`/`triggers` glob matches nothing in the live catalog | warning (the worker may simply be offline — see [discovery](discovery-and-types.md#the-catalog-is-live)) |
| `language == go` | `E_LANG_UNSUPPORTED` (hard) |
