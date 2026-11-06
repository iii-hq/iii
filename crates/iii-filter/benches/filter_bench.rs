//! `cargo bench -p iii-filter`. Budget (observe spec, README §8): a local
//! call with 10 observers, none matching, stays under 5 µs.
//!
//! `index/eq_1_of_<n>` and `scan/eq_1_of_<n>`: `n` filters
//! `{"input.k": {"$eq": i}}` (the load test's `filters` scenario), one of
//! which matches each document, through a [`FilterIndex`] and by evaluating
//! every filter.
//!
//! `index/paths_all_hit/<n>` and `scan/paths_all_hit/<n>`: `n` filters
//! `{"input.k<i>": {"$eq": 1}}`, each on its own path, and a document that
//! meets every one: the index reads one list per path, and merges `n`
//! lists of one candidate each in key order.

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use iii_filter::{Doc, Filter, FilterIndex};
use serde_json::{Value, json};

fn row_change() -> Value {
    json!({
        "db": "main",
        "table": "invoices",
        "op": "update",
        "affected_rows": 3,
        "returning": (0..50).map(|i| json!({
            "id": i,
            "status": if i == 49 { "paid" } else { "open" },
            "amount": i * 25,
            "customer": { "id": format!("c{i}"), "tier": "standard" }
        })).collect::<Vec<_>>()
    })
}

fn observers() -> Vec<Filter> {
    let sources = [
        json!({ "input.op": { "$eq": "insert" } }),
        json!({ "input.table": { "$eq": "orders" }, "input.op": { "$in": ["insert", "update"] } }),
        json!({ "input.affected_rows": { "$gt": 100 } }),
        json!({ "caller.parent.worker_name": { "$eq": "search-indexer" } }),
        json!({ "input.db": { "$prefix": "analytics" } }),
        json!({ "$or": [ { "input.op": { "$eq": "delete" } }, { "error": { "$exists": true } } ] }),
        json!({ "input.returning": { "$any": { "@.status": { "$eq": "void" } } } }),
        json!({ "input.returning.0.customer.tier": { "$eq": "gold" } }),
        json!({ "$not": { "input.table": { "$eq": "invoices" } } }),
        json!({ "metadata.replay": { "$eq": true } }),
    ];
    sources
        .iter()
        .map(|s| Filter::parse(s).expect("valid"))
        .collect()
}

fn bench(c: &mut Criterion) {
    let input = row_change();
    let caller = json!({ "worker_id": "w", "worker_name": "billing" });
    let doc = Doc {
        input: Some(&input),
        caller: Some(&caller),
        ..Doc::default()
    };

    let filters = observers();
    assert!(filters.iter().all(|f| !f.matches(&doc)));
    c.bench_function("10 observers, none matching", |b| {
        b.iter(|| {
            filters
                .iter()
                .filter(|f| f.matches(black_box(&doc)))
                .count()
        })
    });

    let paid = Filter::parse(&json!({
        "input.returning": { "$any": { "@.status": { "$eq": "paid" }, "@.amount": { "$gte": 500 } } }
    }))
    .expect("valid");
    assert!(paid.matches(&doc));
    c.bench_function("$any over 50 rows, last one matching", |b| {
        b.iter(|| paid.matches(black_box(&doc)))
    });

    let src = json!({
        "$or": [ { "input.amount": { "$gt": 500 }, "error": { "$exists": true } },
                 { "error.code": { "$eq": "TIMEOUT" } } ],
        "caller.parent.worker_name": { "$ne": "search-indexer" }
    });
    c.bench_function("parse + normalize", |b| {
        b.iter(|| Filter::parse(black_box(&src)))
    });

    for n in [16u64, 1_024, 4_000] {
        let filters: Vec<Filter> = (0..n)
            .map(|i| Filter::parse(&json!({ "input.k": { "$eq": i } })).expect("valid"))
            .collect();
        let mut index = FilterIndex::new();
        for (i, filter) in filters.iter().enumerate() {
            index.insert(i as u64, filter, i as u64);
        }
        let inputs: Vec<Value> = (0..n).map(|k| json!({ "k": k })).collect();
        let doc_of = |i: u64| Doc {
            input: Some(&inputs[(i.wrapping_mul(2_654_435_761) % n) as usize]),
            ..Doc::default()
        };
        c.bench_with_input(BenchmarkId::new("index/eq_1_of", n), &n, |b, _| {
            let mut i = 0u64;
            b.iter(|| {
                i += 1;
                index.matches(black_box(&doc_of(i))).count()
            })
        });
        c.bench_with_input(BenchmarkId::new("scan/eq_1_of", n), &n, |b, _| {
            let mut i = 0u64;
            b.iter(|| {
                i += 1;
                let doc = doc_of(i);
                filters
                    .iter()
                    .filter(|f| f.matches(black_box(&doc)))
                    .count()
            })
        });
    }
    for n in [4usize, 16, 128] {
        let filters: Vec<Filter> = (0..n)
            .map(|i| Filter::parse(&json!({ format!("input.k{i}"): { "$eq": 1 } })).expect("valid"))
            .collect();
        let mut index = FilterIndex::new();
        for (i, filter) in filters.iter().enumerate() {
            index.insert(i as u64, filter, i as u64);
        }
        let input: Value = (0..n)
            .map(|i| (format!("k{i}"), json!(1)))
            .collect::<serde_json::Map<_, _>>()
            .into();
        let doc = Doc {
            input: Some(&input),
            ..Doc::default()
        };
        c.bench_with_input(BenchmarkId::new("index/paths_all_hit", n), &n, |b, _| {
            b.iter(|| index.matches(black_box(&doc)).count())
        });
        c.bench_with_input(BenchmarkId::new("scan/paths_all_hit", n), &n, |b, _| {
            b.iter(|| {
                filters
                    .iter()
                    .filter(|f| f.matches(black_box(&doc)))
                    .count()
            })
        });
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
