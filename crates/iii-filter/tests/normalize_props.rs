//! Property tests on a seeded random corpus: normalization is idempotent,
//! the normal form is always accepted, and it matches exactly the documents
//! the original filter matches.

#[path = "support/gen.rs"]
mod generator;

use generator::Rng;
use iii_filter::{Filter, limits, normalize};
use serde_json::{Value, json};

const FILTERS: u64 = 2_000;
const DOCS_PER_FILTER: usize = 24;

#[test]
fn normalize_is_idempotent_and_preserves_matching() {
    let mut accepted = 0;
    let (mut matched, mut unmatched) = (0usize, 0usize);
    for seed in 0..FILTERS {
        let mut rng = Rng::new(seed);
        let src = generator::filter(&mut rng);
        let Ok(original) = Filter::parse(&src) else {
            continue;
        };
        accepted += 1;

        let normalized = original.normalized().clone();
        let reparsed = Filter::from_normalized(&normalized)
            .unwrap_or_else(|e| panic!("seed {seed}: normal form {normalized} rejected: {e}"));
        assert_eq!(
            reparsed.normalized(),
            &normalized,
            "seed {seed}: not idempotent for {src}"
        );
        assert_eq!(normalize(&normalized).unwrap(), normalized, "seed {seed}");
        assert_eq!(original.roots(), reparsed.roots(), "seed {seed}");
        assert_eq!(
            original.is_match_all(),
            normalized == json!({}),
            "seed {seed}"
        );

        for _ in 0..DOCS_PER_FILTER {
            let doc = generator::doc(&mut rng, &src);
            let expect = original.matches_value(&doc);
            assert_eq!(
                reparsed.matches_value(&doc),
                expect,
                "seed {seed}: filter {src}, normal form {normalized}, doc {doc}"
            );
            if expect {
                matched += 1;
            } else {
                unmatched += 1;
            }
        }
    }
    assert!(
        accepted * 10 >= FILTERS * 9,
        "only {accepted} of {FILTERS} generated filters were valid"
    );
    // The corpus must exercise both outcomes.
    let total = matched + unmatched;
    assert!(
        matched * 5 >= total && unmatched * 5 >= total,
        "matched {matched} of {total}"
    );
}

#[test]
fn evaluation_is_total_on_arbitrary_documents() {
    let odd_docs = [
        json!(null),
        json!([]),
        json!("input"),
        json!({ "input": null, "output": [], "error": "x", "caller": 1, "metadata": true }),
        json!({ "input": [[[]]], "caller": { "parent": null } }),
    ];
    for seed in 0..500 {
        let mut rng = Rng::new(seed);
        let src = generator::filter(&mut rng);
        let Ok(filter) = Filter::parse(&src) else {
            continue;
        };
        for doc in &odd_docs {
            let _ = filter.matches_value(doc);
        }
    }
}

#[test]
fn size_is_measured_on_the_filter_as_written() {
    // Fits as written, but not once every clause is wrapped in its own object
    // inside an explicit `$and`: still valid, and the normal form compiles
    // through `from_normalized`.
    let mut src = serde_json::Map::new();
    let value = "v".repeat(232);
    for i in 0..32 {
        src.insert(format!("input.k{i:02}"), json!({ "$eq": value }));
    }
    let src = Value::Object(src);
    assert!(serde_json::to_vec(&src).unwrap().len() <= limits::MAX_ENCODED_BYTES);
    let filter = Filter::parse(&src).expect("fits as written");
    let normalized = filter.normalized().clone();
    assert!(serde_json::to_vec(&normalized).unwrap().len() > limits::MAX_ENCODED_BYTES);

    assert!(
        Filter::parse(&normalized).is_err(),
        "the size limit applies to what is parsed"
    );
    let compiled = Filter::from_normalized(&normalized).expect("normal form");
    assert_eq!(compiled.normalized(), &normalized);
    assert_eq!(compiled, filter);
    let doc = json!({ "input": { "k00": value } });
    assert!(!filter.matches_value(&doc) && !compiled.matches_value(&doc));
}

#[test]
fn from_normalized_applies_every_other_rule() {
    for invalid in [
        json!({ "input.a": "literal" }),
        json!({ "$and": [] }),
        json!({ "input.a": { "$eq": 9_007_199_254_740_994u64 } }),
        json!({ "input.a": { "$in": (0..257).collect::<Vec<_>>() } }),
    ] {
        assert!(Filter::from_normalized(&invalid).is_err(), "{invalid}");
    }
}

#[test]
fn and_nesting_is_transparent_for_depth() {
    let mut f = json!({ "input.a": { "$eq": 1 } });
    for _ in 0..30 {
        f = json!({ "$and": [f] });
    }
    let parsed = Filter::parse(&f).unwrap();
    assert_eq!(
        parsed.normalized(),
        &json!({ "$and": [ { "input.a": { "$eq": 1 } } ] })
    );
}

#[test]
fn generator_is_deterministic() {
    let a: Vec<Value> = (0..20)
        .map(|s| {
            let mut rng = Rng::new(s);
            let f = generator::filter(&mut rng);
            json!([f.clone(), generator::doc(&mut rng, &f)])
        })
        .collect();
    let b: Vec<Value> = (0..20)
        .map(|s| {
            let mut rng = Rng::new(s);
            let f = generator::filter(&mut rng);
            json!([f.clone(), generator::doc(&mut rng, &f)])
        })
        .collect();
    assert_eq!(a, b);
}

#[test]
fn deep_inputs_fit_a_small_stack() {
    // On a 2 MiB stack (the default for spawned threads and tokio workers),
    // in a debug build, nothing recurses with the input's depth: the size
    // and the raw nesting (MAX_JSON_DEPTH) are checked with explicit stacks
    // before anything else, for `parse` and `from_normalized` alike.
    // The inputs are leaked: serde_json drops deep values recursively.
    let handle = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let deep_and = |levels: usize| {
                let mut and = json!({ "input.a": { "$eq": 1 } });
                for _ in 0..levels {
                    let mut map = serde_json::Map::new();
                    map.insert("$and".to_owned(), Value::Array(vec![and]));
                    and = Value::Object(map);
                }
                and
            };
            let deep_literal = |levels: usize| {
                let mut literal = json!(0);
                for _ in 0..levels {
                    literal = Value::Array(vec![literal]);
                }
                let mut eq = serde_json::Map::new();
                eq.insert("$eq".to_owned(), literal);
                let mut src = serde_json::Map::new();
                src.insert("input.a".to_owned(), Value::Object(eq));
                Value::Object(src)
            };

            // 2 + 2 × 59 = 120 levels: accepted; one more `$and` is not.
            let and = deep_and(59);
            let filter = Filter::parse(&and).expect("120 levels");
            assert!(filter.matches_value(&json!({ "input": { "a": 1 } })));
            let and = deep_and(60);
            let err = Filter::parse(&and).unwrap_err();
            assert!(err.message.contains("nests deeper than 120"), "{err}");

            // Far deeper: rejected without recursion, size limit or not.
            for value in [deep_and(200_000), deep_literal(200_000)] {
                assert!(Filter::parse(&value).is_err());
                let err = Filter::from_normalized(&value).unwrap_err();
                assert!(err.message.contains("nests deeper than 120"), "{err}");
                std::mem::forget(value);
            }

            let literal = deep_literal(limits::MAX_LITERAL_DEPTH);
            let filter = Filter::parse(&literal).expect("literal at the limit");
            assert!(filter.matches_value(&json!({ "input": { "a": literal["input.a"]["$eq"] } })));
        })
        .unwrap();
    handle.join().expect("no stack overflow");
}

#[test]
fn normal_forms_stay_below_the_json_depth_limit() {
    // The deepest normal form a valid filter can have: seven `$any` filters
    // nested in one another (each level adds a filter object, its `$and`
    // array, a clause object and an operator object), then an `$in` item at
    // the literal depth limit.
    let mut literal = json!(0);
    for _ in 0..limits::MAX_LITERAL_DEPTH {
        literal = json!([literal]);
    }
    let mut filter = json!({ "@": { "$in": [literal] } });
    for _ in 0..6 {
        filter = json!({ "@": { "$any": filter } });
    }
    let src = json!({ "input.a": { "$any": filter } });
    let parsed = Filter::parse(&src).expect("valid");

    fn depth(value: &Value) -> usize {
        match value {
            Value::Array(items) => 1 + items.iter().map(depth).max().unwrap_or(0),
            Value::Object(map) => 1 + map.values().map(depth).max().unwrap_or(0),
            _ => 0,
        }
    }
    let normal_depth = depth(parsed.normalized());
    assert!(normal_depth > depth(&src));
    assert!(
        normal_depth <= 80,
        "normal form nests {normal_depth} levels"
    );
    assert!(Filter::from_normalized(parsed.normalized()).is_ok());
}
