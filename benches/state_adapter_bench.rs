mod common;

use std::sync::Arc;

use criterion::{Criterion, criterion_group, criterion_main};
use iii::modules::state::adapters::{StateAdapter, kv_store::BuiltinKvStoreAdapter};
use iii_sdk::{FieldPath, UpdateOp};
use serde_json::json;
use tokio::runtime::Runtime;

fn state_crud_benchmark(c: &mut Criterion) {
    let rt = Runtime::new().expect("create tokio runtime");
    let adapter = Arc::new(BuiltinKvStoreAdapter::new(None));

    // Pre-populate for get/list benchmarks
    rt.block_on(async {
        adapter
            .set("bench-scope", "existing-key", common::state_value())
            .await
            .unwrap();
        for i in 0..100 {
            adapter
                .set("bench-list", &format!("key-{i}"), common::state_value())
                .await
                .unwrap();
        }
    });

    c.bench_function("state_adapter/set", |b| {
        let adapter = adapter.clone();
        b.to_async(&rt).iter(|| {
            let adapter = adapter.clone();
            async move {
                adapter
                    .set("bench-scope", "key-0", common::state_value())
                    .await
                    .unwrap();
            }
        });
    });

    c.bench_function("state_adapter/get_hit", |b| {
        let adapter = adapter.clone();
        b.to_async(&rt).iter(|| {
            let adapter = adapter.clone();
            async move {
                let result = adapter.get("bench-scope", "existing-key").await.unwrap();
                assert!(result.is_some());
            }
        });
    });

    c.bench_function("state_adapter/get_miss", |b| {
        let adapter = adapter.clone();
        b.to_async(&rt).iter(|| {
            let adapter = adapter.clone();
            async move {
                let result = adapter.get("bench-scope", "nonexistent").await.unwrap();
                assert!(result.is_none());
            }
        });
    });

    c.bench_function("state_adapter/delete", |b| {
        let adapter = adapter.clone();
        b.to_async(&rt).iter(|| {
            let adapter = adapter.clone();
            async move {
                adapter.delete("bench-scope", "key-0").await.unwrap();
            }
        });
    });

    c.bench_function("state_adapter/update_increment", |b| {
        let adapter = adapter.clone();
        // Ensure key exists for updates
        rt.block_on(adapter.set("bench-scope", "update-key", json!({"counter": 0})))
            .unwrap();
        b.to_async(&rt).iter(|| {
            let adapter = adapter.clone();
            async move {
                adapter
                    .update(
                        "bench-scope",
                        "update-key",
                        vec![UpdateOp::Increment {
                            path: FieldPath("counter".to_string()),
                            by: 1,
                        }],
                    )
                    .await
                    .unwrap();
            }
        });
    });

    c.bench_function("state_adapter/list_100", |b| {
        let adapter = adapter.clone();
        b.to_async(&rt).iter(|| {
            let adapter = adapter.clone();
            async move {
                let items = adapter.list("bench-list").await.unwrap();
                assert_eq!(items.len(), 100);
            }
        });
    });

    c.bench_function("state_adapter/list_groups", |b| {
        let adapter = adapter.clone();
        b.to_async(&rt).iter(|| {
            let adapter = adapter.clone();
            async move {
                let groups = adapter.list_groups().await.unwrap();
                assert!(groups.len() >= 2);
            }
        });
    });
}

criterion_group!(benches, state_crud_benchmark);
criterion_main!(benches);
