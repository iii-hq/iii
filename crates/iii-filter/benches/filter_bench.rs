//! `cargo bench -p iii-filter`. Budget (observe spec, README §8): a local
//! call with 10 observers, none matching, stays under 5 µs.

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use iii_filter::{Doc, Filter};
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
}

criterion_group!(benches, bench);
criterion_main!(benches);
