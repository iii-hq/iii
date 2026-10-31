# iii-filter

The iii observer filter language: parse, validate, normalize and evaluate the
`filters` of an observer (`iii.observe(source, target)`).

```rust
use iii_filter::{Doc, Filter};
use serde_json::json;

let filter = Filter::parse(&json!({
    "input.amount": { "$gte": 500 },
    "caller.parent.worker_name": { "$ne": "search-indexer" },
}))
.expect("valid filter");

let input = json!({ "amount": 900 });
assert!(filter.matches(&Doc { input: Some(&input), ..Doc::default() }));
```

- `Filter::parse` validates (operators, roots, limits, no literal shorthand),
  normalizes and compiles a filter. Errors carry the wire code
  `INVALID_FILTER`.
- `Filter::matches` evaluates against a borrowed `Doc` (`input`, `output`,
  `error`, `caller`, `metadata`). It never fails and never clones the
  payload.
- `Filter::normalized` is the normal form the engine stores and sends to
  workers; `normalize` returns it directly. Workers compile it with
  `Filter::from_normalized`, which applies every rule except the encoded-size
  limit (defined on the filter as written; the normal form can be longer).
- `Filter::check_schema_paths` checks `input.*` / `output.*` paths against a
  function's `request_format` / `response_format` (`INVALID_FILTER_PATH`, or
  warnings for paths an open schema cannot verify).
- `FilterIndex` matches many filters against one document. It files each
  filter under one `$eq` / `$in` condition of its top-level `$and` (`path ->
  value -> filters`) and scans the filters without one, so a match costs
  about one lookup per indexed path plus one evaluation per candidate. Its
  matches are exactly the filters `Filter::matches` accepts, in key order.
  It is persistent: a clone is `O(1)` and shares every node, and an insert or
  a removal copies `O(log n)` nodes, so a reader keeps the version it holds.

```rust
use iii_filter::{Doc, Filter, FilterIndex};
use serde_json::json;

let mut index = FilterIndex::new();
for k in 0..4_000u64 {
    let filter = Filter::parse(&json!({ "input.k": { "$eq": k } })).expect("valid filter");
    index.insert(k, &filter, k);
}
let input = json!({ "k": 17 });
let doc = Doc { input: Some(&input), ..Doc::default() };
assert_eq!(index.matches(&doc).map(|(key, _)| *key).collect::<Vec<_>>(), [17]);
```

The engine and every iii SDK implement the same language and run the same
conformance vectors (`sdk/fixtures/filters/` in the
[iii repository](https://github.com/iii-hq/iii)).

License: Apache-2.0.
