# Emitters

An emitter turns the selected, named, type-mapped catalog
([discovery-and-types.md](discovery-and-types.md)) into a single source file. One
emitter per language; all three obey the same skeleton:

```
<banner>
<imports>                 # SDK client type + iii_instance import (if any)
<type declarations>       # all modes (functions/triggers imply types)
<function wrappers>       # mode: functions — grouped into per-worker namespace objects
<trigger registration helpers>   # mode: triggers — methods on the same namespaces
```

Wrappers are grouped by worker into a **namespace object** — `harness::send`
is emitted as `harness.send`, not a flat `send`
([naming](discovery-and-types.md#namespaces--wrapper-names)). The two non-trivial
lowerings — **how a wrapper calls a function** and **how it gets the client** —
are pinned to the real SDK signatures below; the examples show one method of one
namespace.

## The `trigger()` lowering

Every function wrapper compiles to the SDK's one call primitive. The signatures
are quoted from source; the wrapper is the thinnest typed shell over them.

### TypeScript

SDK: `trigger<TInput, TOutput>(request: TriggerRequest<TInput>): Promise<TOutput>`
(`iii/sdk/packages/node/iii/src/types.ts:159`). The generics are already typed,
so the wrapper just supplies them:

```typescript
export const harness = {
  async send(input: SendInput): Promise<SendResult> {
    return iii.trigger<SendInput, SendResult>({
      function_id: 'harness::send',
      payload: input,
    })
  },
  // ...other harness::* functions and trigger helpers share this object
}
```

A sub-namespace nests: `email::accounts::list` →
`export const email = { accounts: { async list(...) { … } } }`.

### Rust

SDK: `pub async fn trigger(&self, request: impl Into<TriggerRequest>) -> Result<Value, Error>`
(`iii/sdk/packages/rust/iii/src/iii.rs:1097-1100`). It takes and returns
`serde_json::Value`, so the wrapper serialises in and deserialises out — this is
the one place the Rust generated code does real work:

```rust
pub mod harness {
    use super::*;
    pub async fn send(iii: &IIIClient, input: SendInput) -> Result<SendResult> {
        let value = iii
            .trigger(TriggerRequest {
                function_id: "harness::send".to_string(),
                payload: serde_json::to_value(input)?,
                action: None,
                timeout_ms: None,
            })
            .await?;
        Ok(serde_json::from_value(value)?)
    }
}
```

The call site is `harness::send(iii, input)` — Rust's native `::` already matches
the id. `use super::*;` brings the file-level type imports and the `Result<T>`
alias into the module; `email::accounts::list` nests as
`pub mod email { pub mod accounts { … } }`.

> `Result<SendResult>` uses a module-level alias the emitter writes once per file
> — `type Result<T> = std::result::Result<T, IIIError>;` — so the signature reads
> exactly as the brief specified (`Result<FunctionResult>`). This requires
> `IIIError: From<serde_json::Error>` for the `?` on `to_value`/`from_value`;
> codegen asserts that conversion exists (the `coder` worker already relies on
> `IIIError::from`, `workers/coder/src/functions/mod.rs:264-276`). `TriggerRequest`
> is `iii_sdk::protocol::TriggerRequest` (`protocol.rs:44`): `{ function_id:
> String, payload: Value, action: Option<TriggerAction>, timeout_ms: Option<u64> }`.

### Python

SDK: `trigger(request)` is **synchronous** and returns the raw response
(`from iii import register_worker`; the Python client passes plain dicts). The
wrapper validates the dict into the Pydantic output model:

```python
class harness:
    @staticmethod
    def send(iii: IIIClient, input: SendInput) -> SendResult:
        result = iii.trigger({
            "function_id": "harness::send",
            "payload": input.model_dump(by_alias=True),
        })
        return SendResult.model_validate(result)
```

The lowercase `class` is a namespace, never instantiated; calls read
`harness.send(iii, input)`. A sub-namespace nests as
`class email: class accounts: ...`.

> No `await` — the Python SDK's `trigger` is sync. `model_dump(by_alias=True)`
> emits the wire field names (matching the JSON Schema property names) even when
> a field was aliased to a Python-safe identifier.

### JavaScript

Same call as TypeScript but emitted as plain async functions; the input/output
types are JSDoc `@typedef`s referenced in `@param`/`@returns`, so editors still
type-check under `checkJs`.

## `iii_instance` lowering

How the wrapper obtains the client it calls `trigger` on, driven by
[configuration § iii_instance](configuration.md#iii_instance).

### `type: import` (TS / JS / Python)

The emitter writes one import using `path` and `name`, and wrappers reference
that symbol — the method signatures above (`harness.send(input)`, no client
parameter) are the `import` form.

| Language | Emitted import (`path: "@/services/iii"`, `name: iii`) |
|---|---|
| TypeScript / JS | `import { iii } from '@/services/iii'` |
| Python | `from services.iii import iii` (dotted module path derived from `path`) |

### `type: argument` (all languages)

No import; the client is the **first parameter** of every wrapper:

| Language | Signature → call site |
|---|---|
| TypeScript | `async send(iii: IIIClient, input: SendInput): Promise<SendResult>` on `harness` → `harness.send(iii, input)` |
| Rust | `pub async fn send(iii: &IIIClient, input: SendInput) -> Result<SendResult>` in `mod harness` → `harness::send(iii, input)` |
| Python | `@staticmethod def send(iii: IIIClient, input: SendInput) -> SendResult` on `harness` → `harness.send(iii, input)` |

The client type is imported from the SDK: `import type { IIIClient } from 'iii-sdk'`
(TS), `use iii_sdk::IIIClient;` (Rust), `from iii import IIIClient` (Python).
(The brief's Rust example wrote `&III`; the SDK's exported type is `IIIClient`,
`iii/sdk/packages/rust/iii/src/iii.rs:685` — codegen emits the real name.)

## Trigger registration helpers

`mode: triggers` emits, per selected trigger type, a helper that **binds a typed
handler** to that trigger — the consumer-side of iii's reactive model. The three
schemas on `TriggerTypeDetail`
([discovery-and-types.md](discovery-and-types.md#exact-response-shapes-verbatim-engine_fnmodrs))
become the three type slots.

```typescript
// trigger type id "email::new-mail" → email.onNewMail(...), on the same `email`
// object as email::* function wrappers
export const email = {
  // ...email::* function wrappers...
  onNewMail(
    config: NewMailConfig,                              // configuration_schema
    handler: (payload: NewMailPayload) => Promise<NewMailReturn>,  // request_schema → response_schema
  ): Trigger {
    const function_id = 'gen::email::new-mail'   // see note on the generated id
    iii.registerFunction(function_id, handler, {
      request_format: /* request_schema */,
      response_format: /* response_schema */,
    })
    return iii.registerTrigger({ type: 'email::new-mail', function_id, config })
  },
}
```

This is the documented **two-step reactive binding** — `registerFunction` then
`registerTrigger` (`iii/sdk/packages/node/iii/src/types.ts:88`,
`workers/todo-worker/src/hooks.ts`). The handler is registered under a
deterministic id in a reserved `gen::<trigger-id>` namespace — an implementation
detail callers never reference, collision-checked within the file. (Binding a
second handler to the same trigger type in one worker takes an explicit
`function_id` override, a v2 helper parameter.) Rust uses
`iii.register_trigger(RegisterTriggerInput { … })` and
Python `iii.register_trigger({ … })` with the same structure; under
`type: argument` the helper takes `iii` as its first parameter, exactly like
function wrappers.

> **Helper name & namespace.** The trigger id nests exactly like a function id:
> the non-leaf segments form the namespace, the leaf becomes `on<PascalLeaf>` on
> that namespace object — `email::new-mail` → `email.onNewMail` /
> `email::on_new_mail`. Built-in trigger types with no `::` (`http`, `cron`) have
> no namespace and emit at the top level as `onHttp` / `onCron`.

`types`-only outputs emit just `NewMailConfig` / `NewMailPayload` /
`NewMailReturn` and no helper.

## File banner

Every generated file opens with a banner so it is unmistakably owned by codegen
and reviewers know not to hand-edit it:

```
// Code generated by iii codegen. DO NOT EDIT.
// source: engine functions [harness::send, harness::status]
//         engine triggers  [harness::job-done]
// codegen v0.1.0
```

(`//` for TS/JS/Rust, `#` for Python.) The banner lists the covered ids so a
diff shows *what* changed in the catalog. It deliberately carries **no
timestamp** — that would make every run dirty (see below).

## Determinism & idempotency

Generated code is committed to repos, so **byte-stability** is a hard
requirement: re-running codegen on an unchanged catalog must produce an identical
file (empty git diff). The emitter guarantees this by:

- **sorting** all selected functions and trigger types by id before emitting;
- **sorting object properties** within a type by their schema property name;
- emitting `$defs` types in sorted order;
- never embedding a timestamp, hostname, or random value.

`codegen generate --check` runs the full pipeline **in memory** and compares
against the files on disk: it writes nothing and exits non-zero if any output
*would* change, reporting each as `status: "would-change"`. This is the CI guard
(catalog drifted but generated code wasn't regenerated) and the formatting
contract — emitted code is pre-formatted to the house style (`biome` for TS,
`rustfmt` conventions for Rust, `ruff` for Python) so a follow-up formatter run
is also a no-op.

## Output writing

For each output the emitter produces the full file text, creates the parent
directory if needed, and (outside `--check`) writes it, recording in the report
`status: "written"` or `"unchanged"` (content identical to what's already there —
no write performed, preserving mtime). The per-output counts (`functions`,
`triggers`, `types`, `bytes`) are tallied for the
[`codegen::generate` response](worker-and-cli.md#codegengenerate).
