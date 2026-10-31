//! Differential tests of [`FilterIndex`] against the evaluator: for seeded
//! random filters and documents, the index matches exactly the entries a scan
//! of every filter with `Filter::matches` matches, in key order, through
//! inserts, removals and replacements, and every version taken along the way
//! keeps matching what it held.
//!
//! Two corpora: the shared generator (`support/gen.rs`: every operator,
//! nesting, `$any`, escaped keys, index segments, numbers near the integer /
//! float boundaries, documents shaped by each filter), and an
//! equality-heavy one written here, where many filters share paths and
//! values and documents respell them (`1` / `1.0` / `-0.0`, `"1"`, `true`,
//! `null`, arrays and objects, absent paths), so that most entries are
//! indexed and most lookups hit.

#[path = "support/gen.rs"]
mod generator;

use generator::Rng;
use iii_filter::{Doc, Filter, FilterIndex};
use serde_json::{Map, Value, json};

/// What an index holds, as `(key, filter)` in key order.
type Model = Vec<(u32, Filter)>;

/// The entries of `filters` (`(key, filter)`, in key order) matching `doc`,
/// by evaluating each.
fn scan(filters: &[(u32, Filter)], doc: &Doc<'_>) -> Vec<u32> {
    filters
        .iter()
        .filter(|(_, filter)| filter.matches(doc))
        .map(|(key, _)| *key)
        .collect()
}

fn indexed(index: &FilterIndex<u32, u32>, doc: &Doc<'_>) -> Vec<u32> {
    index
        .matches(doc)
        .map(|(key, value)| {
            assert_eq!(key, value, "the value filed with the key");
            *key
        })
        .collect()
}

/// Asserts the index and the scan agree on `doc`; returns how many filters
/// the index evaluated.
fn agree(
    index: &FilterIndex<u32, u32>,
    model: &[(u32, Filter)],
    doc_value: &Value,
    context: &str,
) -> usize {
    let doc = Doc::from_value(doc_value);
    let expected = scan(model, &doc);
    let got = indexed(index, &doc);
    if got != expected {
        let filters: Vec<String> = model
            .iter()
            .filter(|(key, _)| expected.contains(key) != got.contains(key))
            .map(|(key, filter)| format!("  {key}: {filter}"))
            .collect();
        panic!(
            "{context}: doc {doc_value}\n  scan  {expected:?}\n  index {got:?}\nfilters that differ:\n{}",
            filters.join("\n")
        );
    }
    index.candidates(&doc)
}

fn valid(rng: &mut Rng) -> (Value, Filter) {
    loop {
        let src = generator::filter(rng);
        if let Ok(filter) = Filter::parse(&src) {
            return (src, filter);
        }
    }
}

#[test]
fn the_index_matches_what_a_scan_matches_on_the_shared_generator() {
    let mut docs = 0usize;
    for seed in 0..200u64 {
        let mut rng = Rng::new(10_000 + seed);
        let count = 8 + rng.below(56);
        let sources: Vec<(Value, Filter)> = (0..count).map(|_| valid(&mut rng)).collect();
        let mut index = FilterIndex::new();
        let mut model: Vec<(u32, Filter)> = Vec::new();
        for (key, (_, filter)) in sources.iter().enumerate() {
            let key = key as u32;
            index.insert(key, filter, key);
            model.push((key, filter.clone()));
        }
        for (src, _) in &sources {
            for _ in 0..6 {
                let doc = generator::doc(&mut rng, src);
                agree(&index, &model, &doc, &format!("seed {seed}"));
                docs += 1;
            }
        }
        // Documents shaped by two filters at once: several indexed paths
        // set, so lookups hit more than one bucket.
        for _ in 0..24 {
            let a = &sources[rng.below(sources.len())].0;
            let b = &sources[rng.below(sources.len())].0;
            let doc = merge(generator::doc(&mut rng, a), generator::doc(&mut rng, b));
            agree(&index, &model, &doc, &format!("seed {seed}, merged"));
            docs += 1;
        }
    }
    assert!(docs > 40_000, "only {docs} documents");
}

/// `b`'s roots over `a`'s.
fn merge(a: Value, b: Value) -> Value {
    let (Value::Object(mut a), Value::Object(b)) = (a, b) else {
        return json!({});
    };
    for (root, value) in b {
        match (a.get_mut(&root), value) {
            (Some(Value::Object(held)), Value::Object(more)) => held.extend(more),
            (_, value) => {
                a.insert(root, value);
            }
        }
    }
    Value::Object(a)
}

/// Values equality filters and documents share: numbers in several
/// spellings, strings that look like them, booleans, `null`, containers.
fn scalar(rng: &mut Rng) -> Value {
    let text = *rng.pick(&[
        "0",
        "-0",
        "0.0",
        "-0.0",
        "1",
        "1.0",
        "1e0",
        "-1",
        "2",
        "2.5",
        "9007199254740992",
        "9007199254740993",
        "-9007199254740992",
        "\"1\"",
        "\"\"",
        "\"a\"",
        "\"A\"",
        "\"é\"",
        "true",
        "false",
        "null",
    ]);
    serde_json::from_str(text).unwrap()
}

/// A literal a filter may hold: a scalar within ±2^53 (`9007199254740993`
/// is a payload number only), sometimes a container.
fn literal(rng: &mut Rng) -> Value {
    if rng.chance(8) {
        return json!([scalar_literal(rng)]);
    }
    if rng.chance(4) {
        return json!({ "k": scalar_literal(rng) });
    }
    scalar_literal(rng)
}

/// A number or a string, as `$gt` / `$gte` / `$lt` / `$lte` take.
fn ordered_literal(rng: &mut Rng) -> Value {
    loop {
        let value = scalar_literal(rng);
        if value.is_number() || value.is_string() {
            return value;
        }
    }
}

fn scalar_literal(rng: &mut Rng) -> Value {
    loop {
        let value = scalar(rng);
        if value.as_u64() != Some(9_007_199_254_740_993) {
            return value;
        }
    }
}

const PATHS: &[&str] = &[
    "input.k",
    "input.k",
    "input.op",
    "input.row.status",
    "input.items.0",
    "output.k",
    "caller.worker_name",
    "metadata.tenant",
    "error.code",
];

/// A filter of one to three top-level clauses on shared paths: mostly
/// `$eq` / `$in`, with residual operators on the same or other paths, and
/// now and then a combinator.
fn equality_filter(rng: &mut Rng) -> Value {
    let mut clauses = Vec::new();
    for _ in 0..1 + rng.below(3) {
        let path = *rng.pick(PATHS);
        let operand = match rng.below(100) {
            0..45 => json!({ "$eq": literal(rng) }),
            45..65 => {
                let items: Vec<Value> = (0..1 + rng.below(4)).map(|_| literal(rng)).collect();
                json!({ "$in": items })
            }
            65..72 => json!({ "$eq": literal(rng), "$ne": literal(rng) }),
            72..78 => json!({ "$in": [], "$exists": true }),
            78..84 => json!({ "$gte": ordered_literal(rng) }),
            84..88 => json!({ "$nin": [literal(rng)] }),
            88..92 => json!({ "$exists": rng.chance(50) }),
            92..96 => json!({ "$any": { "$eq": literal(rng) } }),
            _ => {
                let other = *rng.pick(PATHS);
                clauses.push(json!({ "$or": [
                    { path: { "$eq": literal(rng) } },
                    { other: { "$eq": literal(rng) } },
                ] }));
                continue;
            }
        };
        let mut clause = Map::new();
        clause.insert(path.to_owned(), operand);
        clauses.push(Value::Object(clause));
    }
    if rng.chance(5) {
        let inner = clauses.pop().unwrap_or_else(|| json!({}));
        clauses.push(json!({ "$not": inner }));
    }
    json!({ "$and": clauses })
}

/// A document setting some of the shared paths to shared values (or to a
/// container, or leaving them absent).
fn equality_doc(rng: &mut Rng) -> Value {
    let mut doc = json!({});
    for path in PATHS {
        if rng.chance(30) {
            continue;
        }
        let value = match rng.below(100) {
            0..80 => scalar(rng),
            80..88 => json!([scalar(rng), scalar(rng)]),
            88..94 => json!({ "k": scalar(rng) }),
            _ => json!([]),
        };
        let segments: Vec<&str> = path.split('.').collect();
        set(&mut doc, &segments, value);
    }
    doc
}

fn set(target: &mut Value, segments: &[&str], value: Value) {
    let Some((first, rest)) = segments.split_first() else {
        *target = value;
        return;
    };
    if !target.is_object() {
        *target = json!({});
    }
    let map = target.as_object_mut().unwrap();
    let child = map.entry(first.to_string()).or_insert_with(|| json!({}));
    if rest.is_empty() {
        *child = value;
    } else if first == &"items" {
        // `input.items.0`: an array half of the time, an object otherwise.
        if !child.is_array() && !child.is_object() {
            *child = json!([]);
        }
        if let Value::Array(items) = child {
            items.clear();
            items.push(value);
        } else {
            set(child, rest, value);
        }
    } else {
        set(child, rest, value);
    }
}

#[test]
fn the_index_matches_what_a_scan_matches_on_shared_paths_and_values() {
    let (mut evaluated, mut held) = (0usize, 0usize);
    for seed in 0..60u64 {
        let mut rng = Rng::new(20_000 + seed);
        let count = 50 + rng.below(250);
        let mut index = FilterIndex::new();
        let mut model: Vec<(u32, Filter)> = Vec::new();
        for key in 0..count as u32 {
            let filter = Filter::parse(&equality_filter(&mut rng)).expect("valid");
            index.insert(key, &filter, key);
            model.push((key, filter));
        }
        for _ in 0..200 {
            let doc = equality_doc(&mut rng);
            evaluated += agree(&index, &model, &doc, &format!("seed {seed}"));
            held += model.len();
        }
    }
    // Most entries are indexed, and a lookup rules most of them out (the
    // values here are few, so many entries share each one).
    assert!(
        evaluated * 3 < held,
        "the index evaluated {evaluated} filters where a scan evaluates {held}"
    );
}

/// Filters on many paths, and documents meeting many of them at once: a
/// match merges more lists than it compares head by head.
#[test]
fn the_index_matches_what_a_scan_matches_when_a_document_meets_many_paths() {
    for seed in 0..40u64 {
        let mut rng = Rng::new(25_000 + seed);
        let paths = 5 + rng.below(40);
        let count = 20 + rng.below(300);
        let mut index = FilterIndex::new();
        let mut model: Vec<(u32, Filter)> = Vec::new();
        for key in 0..count as u32 {
            let path = format!("input.p{}", rng.below(paths));
            let other = format!("input.p{}", rng.below(paths));
            let src = match rng.below(100) {
                0..50 => json!({ path: { "$eq": scalar_literal(&mut rng) } }),
                50..70 => {
                    json!({ path: { "$in": [scalar_literal(&mut rng), scalar_literal(&mut rng)] } })
                }
                70..85 => {
                    json!({ path: { "$eq": scalar_literal(&mut rng) }, other: { "$exists": true } })
                }
                _ => json!({ path: { "$exists": rng.chance(80) } }),
            };
            let filter = Filter::parse(&src).expect("valid");
            index.insert(key, &filter, key);
            model.push((key, filter));
        }
        for _ in 0..100 {
            let mut input = Map::new();
            for path in 0..paths {
                if rng.chance(85) {
                    input.insert(format!("p{path}"), scalar(&mut rng));
                }
            }
            let doc = json!({ "input": input });
            agree(&index, &model, &doc, &format!("seed {seed}"));
        }
    }
}

#[test]
fn inserts_removals_and_replacements_keep_the_index_exact_and_every_version_intact() {
    for seed in 0..40u64 {
        let mut rng = Rng::new(30_000 + seed);
        let mut index: FilterIndex<u32, u32> = FilterIndex::new();
        let mut model: Vec<(u32, Filter)> = Vec::new();
        let mut versions: Vec<(FilterIndex<u32, u32>, Model)> = Vec::new();
        for step in 0..600u32 {
            let key = rng.below(120) as u32;
            let at = model.binary_search_by_key(&key, |(k, _)| *k);
            if rng.chance(35) {
                // A removal (of a held key or a missing one).
                let removed = index.remove(&key);
                match at {
                    Ok(at) => {
                        model.remove(at);
                        assert_eq!(removed, Some(key), "seed {seed} step {step}");
                    }
                    Err(_) => assert_eq!(removed, None, "seed {seed} step {step}"),
                }
            } else {
                // An insert, or a replacement under a held key.
                let filter = if rng.chance(50) {
                    Filter::parse(&equality_filter(&mut rng)).expect("valid")
                } else {
                    valid(&mut rng).1
                };
                let replaced = index.insert(key, &filter, key);
                match at {
                    Ok(at) => {
                        assert_eq!(replaced, Some(key));
                        model[at].1 = filter;
                    }
                    Err(at) => {
                        assert_eq!(replaced, None);
                        model.insert(at, (key, filter));
                    }
                }
            }
            assert_eq!(index.len(), model.len());
            let keys: Vec<u32> = index.iter().map(|(k, _)| *k).collect();
            let expected: Vec<u32> = model.iter().map(|(k, _)| *k).collect();
            assert_eq!(keys, expected, "seed {seed} step {step}");
            for _ in 0..3 {
                let doc = equality_doc(&mut rng);
                agree(&index, &model, &doc, &format!("seed {seed} step {step}"));
            }
            if step % 50 == 0 {
                versions.push((index.clone(), model.clone()));
            }
        }
        for (n, (version, model)) in versions.iter().enumerate() {
            for _ in 0..20 {
                let doc = equality_doc(&mut rng);
                agree(version, model, &doc, &format!("seed {seed} version {n}"));
            }
        }
    }
}
