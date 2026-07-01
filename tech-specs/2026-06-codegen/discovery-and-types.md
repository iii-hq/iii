# Discovery & type mapping

This is where codegen's correctness lives. The **input** is the engine's
introspection catalog — already JSON Schema. The **transform** is a deterministic
mapping from JSON Schema to a target language's type system. Everything else
(selection, emission) is plumbing around these two facts.

## Discovery: the input contract

Discovery is not a bespoke "discover" command; it is a set of built-in functions
on the engine itself, invoked like any other function via `iii.trigger`
(`iii/engine/src/workers/engine_fn/mod.rs`). Codegen uses these:

| Function | Input | Returns | Used for |
|---|---|---|---|
| `engine::functions::list` | `{ search?, prefix?, worker?, include_internal? }` | `{ functions: FunctionSummary[] }` | enumerate the catalog |
| `engine::functions::info` | `{ function_id }` | `FunctionDetail` | get a function's request/response **schema** |
| `engine::triggers::list` | `{}` | `TriggerTypeSummary[]` | enumerate trigger types |
| `engine::triggers::info` | `{ id }` | `TriggerTypeDetail` | get a trigger's config/payload/return **schemas** |

(The full surface — also `engine::workers::list/info` and
`engine::registered-triggers::list/info` — is the same one
[`rbac-proxy`](../2026-06-rbac-proxy-worker/engine-overrides.md) filters, and the
`EngineFunctions` enum at `iii/sdk/packages/rust/iii/src/engine.rs:15-25`.
Codegen needs only the four above for generation.)

### Exact response shapes (verbatim, `engine_fn/mod.rs`)

```rust
// engine_fn/mod.rs:170-181  — from engine::functions::list
pub struct FunctionSummary {
    pub function_id: String,
    pub worker_name: String,
    pub description: Option<String>,
    pub metadata: Option<Value>,          // metadata.internal == true → hidden by default
}

// engine_fn/mod.rs:190-203  — from engine::functions::info
pub struct FunctionDetail {
    pub function_id: String,
    pub worker_name: String,
    pub description: Option<String>,
    pub request_schema: Option<Value>,    // <-- JSON Schema (schemars / Draft 7), or None
    pub response_schema: Option<Value>,   // <-- JSON Schema, or None
    pub metadata: Option<Value>,
    pub registered_triggers: Vec<RegisteredTriggerRef>,
}

// engine_fn/mod.rs:213-227  — from engine::triggers::info
pub struct TriggerTypeDetail {
    pub id: String,
    pub worker_name: String,
    pub description: String,
    pub configuration_schema: Option<Value>,  // <-- config the binding takes
    pub request_schema: Option<Value>,         // <-- payload the bound handler RECEIVES
    pub response_schema: Option<Value>,        // <-- value the bound handler RETURNS
    pub instance_count: usize,
}
```

The three schema fields on a trigger type map cleanly onto a registration
helper's three type slots — `configuration_schema` → the helper's `config`
parameter, `request_schema` → the handler's argument, `response_schema` → the
handler's return. See [emitters.md § Trigger helpers](emitters.md#trigger-registration-helpers).

### The schemas are real JSON Schema

`request_schema` / `response_schema` are `serde_json::Value` holding standard
JSON Schema (Draft 7) — the output of `schemars::schema_for!` for Rust workers
(`iii/engine/function-macros/src/lib.rs:358-376`) and of `z.toJSONSchema()` /
hand-written objects for Node workers
(`iii/sdk/packages/node/iii/src/iii-types.ts:56-82`). There is **no custom
dialect**: codegen relies only on `type`, `properties`, `required`, `items`,
`enum`, `oneOf`/`anyOf`/`allOf`, `$ref`, `$defs`/`definitions`,
`additionalProperties`, `format`, `title`, and `description`.

### The catalog is live

> **Codegen can only generate against workers that are connected right now.**
> Discovery reflects the engine's in-memory registry; a worker that isn't running
> contributes nothing. A `functions`/`triggers` glob that matches nothing
> therefore produces a **warning, not an error** — the target may simply be
> offline. Generating from a static, checked-in catalog snapshot (so CI needn't
> boot every worker) is a deliberate v2 item, not v1
> ([Boundaries](worker-and-cli.md#boundaries--non-goals)).

A function with `request_schema: null` / `response_schema: null` (a worker that
registered an untyped `Value` handler) is **not** skipped — it maps to the
language's open type (see the passthrough row below), so the wrapper still
exists, just untyped on that side.

## JSON Schema → language types

One mapping, three targets. The mapper walks a schema, emitting a named type for
the root and for every entry in `$defs`/`definitions`, and resolving `$ref` to
those names.

| JSON Schema | TypeScript | Rust | Python (Pydantic v2) |
|---|---|---|---|
| `{"type":"object","properties":{…},"required":[…]}` | `interface { … }` | `struct { … }` (`#[derive(Serialize,Deserialize,JsonSchema)]`) | `class(BaseModel): …` |
| property **in** `required` | `field: T` | `pub field: T` | `field: T` |
| property **not** in `required` | `field?: T` | `pub field: Option<T>` | `field: T \| None = None` |
| `{"type":"string"}` | `string` | `String` | `str` |
| `{"type":"integer"}` | `number` | `i64` | `int` |
| `{"type":"number"}` | `number` | `f64` | `float` |
| `{"type":"boolean"}` | `boolean` | `bool` | `bool` |
| `{"type":"array","items":S}` | `S[]` | `Vec<S>` | `list[S]` |
| `{"type":"object","additionalProperties":S}` (no `properties`) | `Record<string, S>` | `HashMap<String, S>` | `dict[str, S]` |
| `{"enum":["a","b"]}` (string enum) | `"a" \| "b"` | `enum { A, B }` (`#[serde(rename)]`) | `Literal["a","b"]` |
| `{"oneOf":[…]}` / `{"anyOf":[…]}` | union `A \| B` | `enum`¹ | `A \| B` |
| `{"$ref":"#/$defs/Foo"}` | `Foo` | `Foo` | `Foo` |
| `{"type":["string","null"]}` / `nullable` | `string \| null` | `Option<String>` | `str \| None` |
| absent / `true` / `{}` (no schema) | `unknown` | `serde_json::Value` | `Any` |
| `format: "date-time"`, `uuid`, … | `string` (kept; `format` recorded in a doc-comment) | `String` | `str` |

¹ Rust unions: a `oneOf` of objects each with a single discriminant property
maps to a `#[serde(tag = "…")]` or untagged `enum` chosen to round-trip the
schemars output that produced it; a `oneOf` of primitives maps to an untagged
`enum`. The emitter mirrors schemars' own conventions so a Rust type generated
from a Rust worker's schema is structurally identical to the worker's original.

**`$defs` / `definitions`.** schemars emits nested types into `$defs` (newer) or
`definitions` (older) and `$ref`s them. Codegen collects both, emits one named
type per entry (deduplicated by name across a file — identical definitions
reused, conflicting same-name definitions suffixed `_2`, `_3` and a warning
raised), and references them by name. The root schema's own type takes its name
from its `title` (see below), not from `$defs`.

**Recursion & cycles.** `$ref` cycles (a type that references itself) are
emitted as-is — all three targets express recursive types natively (`Box<T>` is
inserted in Rust where a struct directly contains itself, to keep it `Sized`).

## Naming derivation

Generated identifiers must be **stable** (a function's wrapper keeps its name
across runs) and **collision-free** within a file.

### Type names

1. If the schema has a `title` (schemars sets it to the originating Rust type
   name, e.g. `InfoInput`, `SendResult`), use it.
2. Otherwise derive from the function/trigger: input → `<Pascal>Input`,
   output → `<Pascal>Result`, trigger config → `<Pascal>Config`, trigger payload
   → `<Pascal>Payload`, trigger return → `<Pascal>Return`. `<Pascal>` is the
   PascalCase of the **leaf method name** (the function/trigger's last `::`
   segment, below) — so `email::new-mail` yields types `NewMailConfig` /
   `NewMailPayload` / `NewMailReturn` and a helper method named `onNewMail`.
3. **Collisions.** Type declarations are top-level (never nested inside a
   namespace), so two workers can derive the same name (`harness::send` and
   `email::send` both → `SendInput`). On any clash within a file, every clashing
   type falls back to a namespace-qualified PascalCase name (`HarnessSendInput`,
   `EmailSendInput`), deterministically; `$defs` types follow the same rule. The
   wrappers themselves never collide — they are namespaced (below).

### Namespaces & wrapper names

A function is **not** emitted as a flat top-level function. Its `function_id`'s
`::` segments map to a **nested namespace**, and the last segment becomes the
method — so a call reads like the id with `::` swapped for the language's member
operator:

| function_id | TypeScript / JavaScript | Python | Rust |
|---|---|---|---|
| `harness::send` | `harness.send(input)` | `harness.send(input)` | `harness::send(input)` |
| `email::accounts::list` | `email.accounts.list()` | `email.accounts.list()` | `email::accounts::list()` |

Each non-leaf segment is a namespace level — a nested object in TS/JS, a nested
class in Python, a nested `pub mod` in Rust (Rust's native `::` makes the call
site match the id verbatim). The leaf method is cased per language: `camelCase`
(TS/JS) / `snake_case` (Python/Rust). Namespace segments are sanitized to valid
identifiers and cased the same way, so `session-manager::get` →
`sessionManager.get` (TS) / `session_manager.get` (Python) /
`session_manager::get` (Rust). A worker's functions **and** its trigger helpers
([emitters.md](emitters.md#trigger-registration-helpers)) share one namespace
object.

Because each worker is its own namespace, **two workers' same-named functions no
longer collide** — `harness.send` and `email.send` coexist, so the flat scheme's
worker-prefix collision rule is gone. The remaining edge cases:

- **A segment that is both a function and a namespace** (e.g. `email::accounts`
  *and* `email::accounts::list`): the namespace wins; the bare function is
  emitted with a trailing `_` and a warning is raised.
- **Non-identifier ids** (`api::get::/todos`): each segment is sanitized (`/` and
  other non-identifier characters dropped), then nested → `api.get.todos` /
  `api::get::todos`. A leading digit is prefixed with `_`.

Nesting is computed once over an output's selected set, so it stays stable and
independent across outputs.
