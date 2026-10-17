# Differential corpus

`corpus.json` holds random valid filters and documents with the verdict of the
Rust evaluator (`crates/iii-filter`), produced by its seeded generator. The
other evaluators must give the same verdict on every case:

- Node, shared with `iii-browser`: `sdk/packages/node/iii/tests/filter-conformance.test.ts`;
- Python: `sdk/packages/python/iii/tests/test_filter_conformance.py`;
- Go: `sdk/packages/go/iii/filter/filter_test.go`.

`crates/iii-filter/tests/diff_corpus.rs` fails when this file differs from
what `diffgen` produces.

Format: `{"runs":[<diffgen output>, ...]}`, where each run is the verbatim
output of one `diffgen` invocation, `{"seed":N,"cases":[{"filters":…,"doc":…,"expect":bool}]}`.
Three runs of 500 cases (seeds 1, 2, 3), about 385 KB.

Regenerate it (from the repository root) with exactly:

```sh
cargo build -p iii-filter --example diffgen
{ printf '{"runs":['; first=1; for s in 1 2 3; do [ $first = 1 ] || printf ','; first=0; ./target/debug/examples/diffgen --seed $s --count 500 | tr -d '\n'; done; printf ']}\n'; } > sdk/fixtures/filters/diff/corpus.json
```

Each run equals `cargo run -p iii-filter --example diffgen -- --seed N --count 500`
for its seed, so a change to the generator or to the Rust evaluator shows up as a
diff of this file. Do not edit it by hand.
