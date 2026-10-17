//! The committed differential corpus (`sdk/fixtures/filters/diff/corpus.json`)
//! is exactly what `diffgen` produces today: a change to the generator or to
//! the evaluator must come with a regenerated corpus.

#[path = "support/gen.rs"]
mod generator;

use std::fs;
use std::path::Path;

/// The runs the corpus holds, as in `sdk/fixtures/filters/diff/README.md`.
const SEEDS: [u64; 3] = [1, 2, 3];
const CASES_PER_RUN: usize = 500;

#[test]
fn committed_corpus_matches_diffgen() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sdk/fixtures/filters/diff/corpus.json");
    let committed =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));

    let runs: Vec<String> = SEEDS
        .iter()
        .map(|&seed| generator::diff_run(seed, CASES_PER_RUN))
        .collect();
    let expected = format!("{{\"runs\":[{}]}}\n", runs.join(","));

    assert!(
        committed == expected,
        "sdk/fixtures/filters/diff/corpus.json is stale: regenerate it with the command in \
         sdk/fixtures/filters/diff/README.md"
    );
}
