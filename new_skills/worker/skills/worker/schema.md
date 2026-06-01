---
type: how-to
function_id: worker::schema
title: Discover request and response schemas for worker ops
---

# When to use

Call `worker::schema` to introspect the JSON Schemas for every
`worker::*` op. Each entry carries field descriptions, defaults, and
types, plus per-op `default_timeout_ms` and `idempotent` hints, so you
can construct payloads without source-diving. It's a pure read — safe
to poll, idempotent.

Reach for it when:

- You're scaffolding a generic worker-management UI and need to
  enumerate every op the daemon exposes.
- A caller hit a validation error and you want to see the exact field
  shape the daemon expects.
- You're writing a typed client and want the schemas as the source of
  truth.

# Inputs

```json
{
  "function_id":  "worker::add"  // optional; omit to list all 8 ops, pass one id to retrieve just that schema
}
```

`function_id` is optional. Omit it to get every op back; pass one id
(e.g. `"worker::add"`) to narrow the response to a single entry.

# Outputs

```json
{
  "schemas": [
    {
      "function_id":         "worker::add",                                       // dotted op id
      "description":         "Install a worker from registry name or OCI ref",  // one-line description matching the op's how-to title
      "request":             { "...": "JSON Schema for AddOptions" },           // full JSON Schema for the request body
      "response":            { "...": "JSON Schema for AddOutcome" },           // full JSON Schema for the response body
      "default_timeout_ms":  600000,                                              // suggested client-side timeout for this op
      "idempotent":          true                                                 // true when calling twice with the same input is safe
    }
  ]
}
```

- `schemas` is sorted lexicographically by `function_id`.
- `description` matches the action-oriented title in each op's how-to
  (e.g. this file's frontmatter `title`).
- `request` / `response` are full JSON Schema objects — feed them
  directly into a validator like `ajv`.
- `default_timeout_ms` is a suggestion, not a daemon-side enforced
  ceiling; callers can pass shorter or longer timeouts at their own
  risk.
- `idempotent: true` means re-issuing the same call is safe; `false`
  flags stateful ops (`worker::start`, `worker::stop`) that should not
  be blindly retried.

# Worked example

Get every op's schema in one call:

```json
{}
```

Or narrow to a single op when you only need that one:

```json
{ "function_id": "worker::add" }
```

# Related

- `worker::list` — pair with the schemas to validate inputs before
  calling other ops.
- `worker::add` / `worker::remove` / `worker::start` / `worker::stop` /
  `worker::update` / `worker::clear` — the ops whose shapes this
  function describes.
