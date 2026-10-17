# Filter language (`iii-filter` v1)

A filter is the `filters` object on the left side of `iii.observe`. It decides
whether one invocation is delivered to one observer. The same filter runs in
up to three places:

- in the engine, for routed invocations of `auto` functions;
- in the SDK that hosts the function, for local invocations of `auto`
  functions;
- in the owner's SDK, for `owner`-delivered functions: inside `obs.emit` and
  `obs.call`, and through `obs.matches(doc)` when the owner wants to know first
  (the HTTP router answers 422 this way, [`http.md`](http.md) §2.3).

All of them must agree on every input. That is why the language is small,
total, has no side effects and no regular expressions, and ships with shared
conformance vectors.

Filters are not arguments. The source's `args` (a cron expression, an HTTP
method and path, a database table) go to the function's owner, which decides
what to set up and which observers each event belongs to; `filters` decide
which of those events reach the target (§4).

## 1. The document a filter sees

```jsonc
{
  "input":    <invocation payload>,          // what the function was called with
  "output":   <result>,                      // present only on success
  "error":    { "code": "...", "message": "..." },  // present only on failure
  "caller":   {
    "worker_id": "uuid", "worker_name": "billing",
    "function_id": "billing::close-invoice", // absent for top-level calls
    "invocation_id": "uuid",
    "parent": { "worker_id": "...", "worker_name": "...", "function_id": "...", "invocation_id": "..." }
  },
  "metadata": <invocation metadata sidecar>
}
```

- A filter may reference any root, whatever the observer captures. Only the
  fields listed in `capture` are shipped to the target.
- `output` and `error` are absent on the failing and the successful path
  respectively.
- `on` is checked before the filter: `success`, `error` or `complete`.
- For `owner`-delivered functions the SDK builds the document from what the
  owner passes to `obs.emit` / `obs.call`: `input`, `caller` (from the
  invocation context at that point) and `metadata`. `output` and `error` do not
  exist yet at that point.
- **The owner never edits filters.** A filter belongs to its observer. An owner
  decides which observers an event is for, from their args; the SDK then
  applies each observer's filters as written ([`sdk.md`](sdk.md) §1).

## 2. Syntax

```text
Filters   := { Clause, ... }                     // implicit AND of all clauses; {} matches everything
Clause    := "<path>": { "$op": operand, ... }   // AND of operators on that path
           | "$and": [Filters, ...]
           | "$or":  [Filters, ...]
           | "$not": Filters
Path      := root ("." segment)*                 // root ∈ input | output | error | caller | metadata; @ inside $any
segment   := key | index                         // index = "0" or a decimal without a leading zero
```

A path clause always takes an **operator object**. There is no literal
shorthand: `{ "input.op": "insert" }` is `INVALID_FILTER`; write
`{ "input.op": { "$eq": "insert" } }`. One form keeps every filter a 1:1
image of an iii lang predicate (§2.2).

- `$and` and `$or` take a non-empty array of filters (`[]` is
  `INVALID_FILTER`, as in MongoDB). `$not` takes a filter; `{}` is allowed
  there and matches nothing.
- A key may hold any character. `\.` (a dot) and `\\` (a backslash) are the
  only escapes; any other backslash, an empty path and an empty segment are
  `INVALID_FILTER`.
- An index is `0` or a decimal without a leading zero. Any other segment is a
  key: `01` and `-1` are keys.
- `@` is a root only inside `$any`, where every path starts at it (§2.1).
  After the root, `@` is an ordinary key.

### 2.1 Operators

| Operator | Operand | Matches when |
|---|---|---|
| `$eq` | any JSON | deep equal |
| `$ne` | any JSON | not deep equal, **or the path is absent** |
| `$in` | array (≤ 256) | deep equal to some element |
| `$nin` | array (≤ 256) | not deep equal to any element, **or the path is absent** |
| `$gt` `$gte` `$lt` `$lte` | number or string | same JSON type and ordered (see §3) |
| `$exists` | bool | presence of the path equals the operand |
| `$prefix` | string | the value is a string that starts with the operand |
| `$any` | Filters or operator object | the value is an array and at least one element matches (see below) |

`$any` matches array elements:

- **Scalar elements:** pass an operator object.
  `{"input.tags": {"$any": {"$eq": "vip"}}}`
- **Object elements:** pass a filter whose paths start at `@`, the current
  element.
  `{"input.returning": {"$any": {"@.status": {"$eq": "paid"}, "@.amount": {"$gte": 500}}}}`

### 2.2 From iii lang

In iii lang the source's call arguments become `args`, a filter stage becomes
`filters`, and `~>` names the target. The filter stage is **planned**; the
syntax below is illustrative, not iii lang v12:

```text
onTick({ expression: '0 0 9 * * Mon-Fri' })
  | 'input.scheduled_time' >= '2026-11-01T00:00:00Z' && 'caller.worker_name' != 'replayer'
  ~> cronHandler
```

```ts
iii.observe({
  function_id: 'cron::on-tick',
  args: { expression: '0 0 9 * * Mon-Fri' },
  filters: { 'input.scheduled_time': { $gte: '2026-11-01T00:00:00Z' },
             'caller.worker_name': { $ne: 'replayer' } },
}, 'local::cron-handler')
```

| iii lang predicate | `filters` |
|---|---|
| `'p' == v` | `{ "p": { "$eq": v } }` |
| `'p' != v` | `{ "p": { "$ne": v } }` |
| `'p' < v`, `<=`, `>`, `>=` | `$lt`, `$lte`, `$gt`, `$gte` |
| `'p' in [a, b]`, `not in` | `$in`, `$nin` |
| `a && b` | two clauses (implicit AND), or `$and` |
| `a \|\| b` | `$or` |
| `!a` | `$not` |

`$exists`, `$prefix` and `$any` have no operator of their own; they map to
functions in the predicate language.

## 3. Semantics

- **Path resolution.** A path yields a value or *absent*.
  - Stepping into a non-container, a missing key or an out-of-range index
    yields absent.
  - An index segment (`0`, `1`, `12`, never `01`) indexes an array, and
    looks up a key on an object. Any other segment is a key, so on an array
    it is absent.
  - Only own keys exist: `constructor` or `__proto__` resolve only when the
    document holds them.
  - There is **no implicit fan-out over arrays** (unlike MongoDB). Use `$any`.
- **Absent.** Only `$exists: false`, `$ne` and `$nin` match an absent path.
  Every other operator is false. `$not` negates the whole sub-filter.
- **Equality** is JSON deep equality.
  - Numbers compare by value (`1 == 1.0`).
  - Object key order does not matter.
  - `null` is a value, not absent.
- **Ordering** applies only within one type.
  - Numbers compare numerically.
  - Strings compare by Unicode code point, which is UTF-8 byte order.
  - Mixed types → false. There is no coercion.
- **Numbers.** Numeric literals outside ±2^53 are rejected at validation,
  floats included (`1e16`), so JavaScript evaluates the same way as Rust.
  Payload integers above 2^53 compare as f64 in every implementation.
- **Host-language values (SDK evaluation).** The SDK must evaluate as if on
  `JSON.parse(JSON.stringify(doc))`:
  - `undefined` counts as absent;
  - `Date` becomes its ISO string;
  - `BigInt` (JS) and `Decimal` (Python) are compared through their JSON
    encoding.

  Implementations map these lazily and do not serialize.
- **Totality.** Evaluation never fails. A filter that is valid at creation
  returns true or false for every document.

## 4. Validation (in the engine, at `Observe`)

- **Structural limits:**
  - nesting depth ≤ 8: the top-level filter is level 1; each `$or` member,
    the `$not` operand and the `$any` operand (both forms) are one level
    deeper than the object holding them; `$and` members stay at the level of
    the object holding the `$and`;
  - ≤ 64 nodes: each operator (`$eq` … `$any`) and each `$or` / `$not`
    counts one; paths and `$and` are free;
  - `$in`/`$nin` ≤ 256 items;
  - ≤ 16 path segments after the root;
  - literal depth ≤ 32: arrays and objects nested in one `$eq` / `$ne`
    operand or one `$in` / `$nin` item (a scalar is 0, `[1]` is 1; the `$in`
    array itself does not count);
  - JSON depth ≤ 120: arrays and objects nested in the whole filter value,
    `$and` wrappers and literals included (the engine's JSON decoder stops at
    128 levels per message); this is what bounds a chain of `$and`s;
  - encoded size ≤ 8 KiB: UTF-8 bytes of the compact JSON of the filter
    **as written**.
- Normalizing (§5) never increases the filter depth, nodes, `$in` items,
  path segments or literal depth. It wraps every clause in its own object, so
  the JSON depth can grow, but the normal form of a valid filter stays far
  below 120 (65 levels at most). The encoded size can grow too, so hosts
  compile the normal forms the engine pushes without the size limit; every
  other rule still applies.
- **Roots and operators.** Unknown roots or operators, or a path clause whose
  value is not an operator object → `INVALID_FILTER`.
- **Schema checks.** When the function registered `request_format` (for
  `input.*`) or `response_format` (for `output.*`), every path is checked
  against the JSON Schema `properties`/`items`.
  - An unknown property under a closed schema (`additionalProperties: false`)
    → `INVALID_FILTER_PATH`.
  - Under an open schema it is a warning on the `ObserveResult`. So is a
    construct the check cannot follow: a remote `$ref`, `anyOf` / `oneOf` /
    `allOf` with several branches, `patternProperties`, `if`. Local `$ref`s
    and nullable wrappers are followed.
  - A path that reaches a `false` schema is `INVALID_FILTER_PATH`.
  - `@` paths inside `$any` are checked against the array's `items`.
  - Schema checks run in the engine only, at `observe`.
  - This replaces the `deny_unknown_fields` protection that trigger configs
    have today: a typo like `input.tabel` fails at creation instead of
    matching nothing forever.
- **Args are not filters.** `args` are validated separately, against the
  function's `observe.args_format` (`INVALID_ARGS`, [`protocol.md`](protocol.md)
  §4). `required` there is how an owner demands an argument: the cron
  `expression`, the HTTP `method` and `path`, the database `db`. Args exist
  only on `owner`-delivered functions, where the owner routes by them; an
  `auto` function takes none. Args are constant for the observer's life, so
  filters cannot reference them.

## 5. Normal form

The engine normalizes every filter before storing it or pushing it to a host:

- every filter, nested ones included (`$or` members, `$not`, the filter form
  of `$any`), becomes `{}` or `{ "$and": [ … ] }` with one clause per object;
- explicit `$and`s are flattened into the filter that holds them;
- clauses are sorted by key in byte order (`$not` and `$or` before paths),
  then by the compact JSON of their operand, and exact duplicates are
  dropped;
- the keys of an operator object are sorted; operands, `$in`/`$nin` items
  and paths are kept as written; `$or` members keep their order;
- `$ne`/`$nin` stay as they are, keeping their absent-matching semantics.

Normalizing is idempotent, and a normal form matches exactly the documents
its source matches.

The normalized form is what `functionregistered` / `observeradded` carry and
what SDKs compile, once per added observer, into a closure tree.

## 6. Examples

```jsonc
// updates that touched more than 100 rows (among the events the owner routes to the observer by its args)
{ "input.op": { "$eq": "update" }, "input.affected_rows": { "$gt": 100 } }

// writes made on behalf of anyone except the search indexer (stops echo loops)
{ "caller.parent.worker_name": { "$ne": "search-indexer" } }

// failed charges above 500, or any failure with a timeout code
{ "$or": [
    { "input.amount": { "$gt": 500 }, "error": { "$exists": true } },
    { "error.code": { "$eq": "TIMEOUT" } } ] }

// a row change touching a paid invoice (native capture reports keys only; statements capture reports RETURNING rows)
{ "input.returning": { "$any": { "@.status": { "$eq": "paid" } } } }
```

## 7. Conformance vectors

`sdk/fixtures/filters/<name>.json`:

```json
{
  "filters": { "input.op": { "$in": ["insert", "update"] }, "caller.worker_name": { "$ne": "indexer" } },
  "cases": [
    { "doc": { "input": { "op": "insert" }, "caller": { "worker_name": "billing" } }, "expect": true },
    { "doc": { "input": { "op": "insert" } }, "expect": true },
    { "doc": { "input": { "op": "delete" }, "caller": { "worker_name": "billing" } }, "expect": false }
  ]
}
```

Invalid filters live in `sdk/fixtures/filters/invalid/` together with the
expected error code (a literal shorthand such as `{ "input.op": "insert" }` is
one of them). Every implementation, engine included, runs the full set in CI.
`normalize/` holds normal-form vectors for the engine (other implementations
only check that each normal form compiles), and `diff/` a differential corpus
generated by the Rust evaluator, which the Node, Python and Go evaluators
replay. The vectors' `README.md` defines the formats and spells out every rule
above that an implementation must reproduce.
