//! Runs the shared conformance vectors in `sdk/fixtures/filters/`.

use std::fs;
use std::path::{Path, PathBuf};

use iii_filter::{Doc, Filter, FilterIndex, normalize};
use serde_json::Value;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sdk/fixtures/filters")
}

fn json_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    files
}

fn load(path: &Path) -> (String, Value) {
    let name = path
        .strip_prefix(fixtures())
        .unwrap_or(path)
        .display()
        .to_string();
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {name}: {e}"));
    let value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parsing {name}: {e}"));
    (name, value)
}

/// `object[key]`, which must be present: a misspelled key would otherwise
/// read as `null` and an invalid case would pass vacuously.
fn field<'v>(object: &'v Value, key: &str, context: &str) -> &'v Value {
    object
        .get(key)
        .unwrap_or_else(|| panic!("{context}: missing `{key}`"))
}

fn cases<'v>(name: &str, file: &'v Value) -> &'v [Value] {
    let cases = file["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("{name}: `cases` must be an array"));
    assert!(!cases.is_empty(), "{name}: no cases");
    cases
}

#[test]
fn valid_vectors() {
    let files = json_files(&fixtures());
    assert!(
        files.len() >= 100,
        "expected the full vector set, found {}",
        files.len()
    );

    let mut failures = Vec::new();
    let mut total = 0;
    for path in files {
        let (name, file) = load(&path);
        assert!(
            file["description"].is_string(),
            "{name}: missing description"
        );
        let filter = match Filter::parse(field(&file, "filters", &name)) {
            Ok(filter) => filter,
            Err(e) => {
                failures.push(format!("{name}: valid filter rejected: {e}"));
                continue;
            }
        };
        // The normal form, as a worker receives it, must compile and evaluate
        // the same way.
        let renormalized = match Filter::from_normalized(filter.normalized()) {
            Ok(f) => f,
            Err(e) => {
                failures.push(format!("{name}: normal form rejected: {e}"));
                continue;
            }
        };
        if renormalized.normalized() != filter.normalized() {
            failures.push(format!("{name}: normalization is not idempotent"));
        }

        for (i, case) in cases(&name, &file).iter().enumerate() {
            total += 1;
            let expect = case["expect"]
                .as_bool()
                .unwrap_or_else(|| panic!("{name} case {i}: `expect` must be a boolean"));
            let doc = case
                .get("doc")
                .unwrap_or_else(|| panic!("{name} case {i}: missing `doc`"));
            let got = filter.matches_value(doc);
            if got != expect {
                failures.push(format!(
                    "{name} case {i}: expected {expect}, got {got} (doc {doc}, note {})",
                    case["note"]
                ));
            }
            if renormalized.matches_value(doc) != expect {
                failures.push(format!("{name} case {i}: the normal form disagrees"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(total >= 500, "only {total} cases");
}

/// Every valid vector's filter in one [`FilterIndex`], each document of
/// every file matched against all of them: the index yields exactly the
/// filters that match it (its own vector's `expect` among them), in key
/// order, whatever the other filters filed beside it.
#[test]
fn valid_vectors_through_one_index() {
    let mut index = FilterIndex::new();
    let mut filters: Vec<Filter> = Vec::new();
    let mut docs: Vec<(String, Value)> = Vec::new();
    for path in json_files(&fixtures()) {
        let (name, file) = load(&path);
        let filter = Filter::parse(field(&file, "filters", &name)).expect("a valid vector");
        let key = filters.len();
        index.insert(key, &filter, key);
        filters.push(filter);
        for (i, case) in cases(&name, &file).iter().enumerate() {
            docs.push((format!("{name} case {i}"), case["doc"].clone()));
        }
    }
    let mut failures = Vec::new();
    for (context, doc) in &docs {
        let doc_view = Doc::from_value(doc);
        let scanned: Vec<usize> = (0..filters.len())
            .filter(|key| filters[*key].matches(&doc_view))
            .collect();
        let got: Vec<usize> = index.matches(&doc_view).map(|(key, _)| *key).collect();
        if got != scanned {
            failures.push(format!(
                "{context}: scan {scanned:?}, index {got:?} (doc {doc})"
            ));
        }
    }
    // Each vector's own expectation, read through the index.
    for (key, path) in json_files(&fixtures()).into_iter().enumerate() {
        let (name, file) = load(&path);
        for (i, case) in cases(&name, &file).iter().enumerate() {
            let expect = case["expect"].as_bool().expect("a boolean `expect`");
            let doc_view = Doc::from_value(&case["doc"]);
            let got = index.matches(&doc_view).any(|(k, _)| *k == key);
            if got != expect {
                failures.push(format!(
                    "{name} case {i}: through the index {got}, expected {expect}"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The differential corpus (`diff/corpus.json`) through one index per run:
/// each document matched against every filter of its run. (Its `expect`
/// values are checked by the other implementations; serde_json reads a few of
/// its float literals one ulp off, so this compares with a scan of the same
/// parsed filters.)
#[test]
fn the_diff_corpus_through_one_index_per_run() {
    let (_, corpus) = load(&fixtures().join("diff/corpus.json"));
    let mut checked = 0;
    for run in corpus["runs"].as_array().expect("runs") {
        let cases = run["cases"].as_array().expect("cases");
        let mut index = FilterIndex::new();
        let mut filters = Vec::new();
        for (key, case) in cases.iter().enumerate() {
            let filter = Filter::parse(&case["filters"]).expect("a corpus filter");
            index.insert(key, &filter, key);
            filters.push(filter);
        }
        for (at, case) in cases.iter().enumerate() {
            let doc = Doc::from_value(&case["doc"]);
            let scanned: Vec<usize> = (0..filters.len())
                .filter(|key| filters[*key].matches(&doc))
                .collect();
            let got: Vec<usize> = index.matches(&doc).map(|(key, _)| *key).collect();
            assert_eq!(got, scanned, "run {} case {at}", run["seed"]);
            checked += 1;
        }
    }
    assert!(checked >= 1_500, "only {checked} cases");
}

#[test]
fn invalid_vectors() {
    let files = json_files(&fixtures().join("invalid"));
    assert!(
        files.len() >= 15,
        "expected the full invalid set, found {}",
        files.len()
    );

    let mut failures = Vec::new();
    for path in files {
        let (name, file) = load(&path);
        assert!(
            file["description"].is_string(),
            "{name}: missing description"
        );
        for (i, case) in cases(&name, &file).iter().enumerate() {
            let code = case["code"]
                .as_str()
                .unwrap_or_else(|| panic!("{name} case {i}: `code` must be a string"));
            let filters = field(case, "filters", &format!("{name} case {i}"));
            match Filter::parse(filters) {
                Ok(f) => failures.push(format!(
                    "{name} case {i} ({}): accepted, normalized to {f}",
                    case["note"]
                )),
                Err(e) if e.code.as_str() != code => {
                    failures.push(format!("{name} case {i}: expected {code}, got {e}"))
                }
                Err(_) => {}
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn normalize_vectors() {
    let files = json_files(&fixtures().join("normalize"));
    assert!(
        files.len() >= 5,
        "expected the normalize set, found {}",
        files.len()
    );

    let mut failures = Vec::new();
    for path in files {
        let (name, file) = load(&path);
        for (i, case) in cases(&name, &file).iter().enumerate() {
            let context = format!("{name} case {i}");
            let expected = field(case, "normalized", &context);
            match normalize(field(case, "filters", &context)) {
                Ok(got) if &got == expected => {
                    // Normal forms are fixed points.
                    match Filter::from_normalized(expected) {
                        Ok(again) if again.normalized() == expected => {}
                        other => failures.push(format!(
                            "{name} case {i}: the normal form renormalizes to {other:?}"
                        )),
                    }
                }
                Ok(got) => failures.push(format!(
                    "{name} case {i}:\n  expected {expected}\n  got      {got}"
                )),
                Err(e) => failures.push(format!("{name} case {i}: rejected: {e}")),
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
