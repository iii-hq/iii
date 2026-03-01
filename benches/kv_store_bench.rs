mod common;

use std::{sync::Arc, time::Instant};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use futures::future::join_all;
use iii::builtins::kv::BuiltinKvStore;
use iii_sdk::{FieldPath, UpdateOp};
use serde_json::json;
use tokio::runtime::Runtime;

fn kv_set_get_benchmark(c: &mut Criterion) {
    let rt = Runtime::new().expect("create tokio runtime");
    let kv = Arc::new(BuiltinKvStore::new(None));

    // Pre-populate a key for GET benchmarks
    rt.block_on(kv.set(
        "bench".to_string(),
        "existing-key".to_string(),
        common::kv_value(),
    ));

    c.bench_function("kv_store/set", |b| {
        let kv = kv.clone();
        b.to_async(&rt).iter(|| {
            let kv = kv.clone();
            async move {
                kv.set("bench".to_string(), "key-0".to_string(), common::kv_value())
                    .await
            }
        });
    });

    c.bench_function("kv_store/get_hit", |b| {
        let kv = kv.clone();
        b.to_async(&rt).iter(|| {
            let kv = kv.clone();
            async move {
                let result = kv
                    .get("bench".to_string(), "existing-key".to_string())
                    .await;
                assert!(result.is_some());
            }
        });
    });

    c.bench_function("kv_store/get_miss", |b| {
        let kv = kv.clone();
        b.to_async(&rt).iter(|| {
            let kv = kv.clone();
            async move {
                let result = kv
                    .get("bench".to_string(), "nonexistent-key".to_string())
                    .await;
                assert!(result.is_none());
            }
        });
    });

    c.bench_function("kv_store/delete", |b| {
        let kv = kv.clone();
        b.to_async(&rt).iter(|| {
            let kv = kv.clone();
            async move {
                kv.delete("bench".to_string(), "key-0".to_string()).await;
            }
        });
    });
}

fn kv_update_benchmark(c: &mut Criterion) {
    let rt = Runtime::new().expect("create tokio runtime");
    let kv = Arc::new(BuiltinKvStore::new(None));

    // Pre-populate for update benchmarks
    rt.block_on(kv.set(
        "bench".to_string(),
        "update-key".to_string(),
        json!({"name": "A", "counter": 0}),
    ));

    c.bench_function("kv_store/update_set_field", |b| {
        let kv = kv.clone();
        b.to_async(&rt).iter(|| {
            let kv = kv.clone();
            async move {
                kv.update(
                    "bench".to_string(),
                    "update-key".to_string(),
                    vec![UpdateOp::Set {
                        path: FieldPath("name".to_string()),
                        value: Some(json!("B")),
                    }],
                )
                .await
            }
        });
    });

    c.bench_function("kv_store/update_increment", |b| {
        let kv = kv.clone();
        b.to_async(&rt).iter(|| {
            let kv = kv.clone();
            async move {
                kv.update(
                    "bench".to_string(),
                    "update-key".to_string(),
                    vec![UpdateOp::Increment {
                        path: FieldPath("counter".to_string()),
                        by: 1,
                    }],
                )
                .await
            }
        });
    });

    c.bench_function("kv_store/update_merge", |b| {
        let kv = kv.clone();
        b.to_async(&rt).iter(|| {
            let kv = kv.clone();
            async move {
                kv.update(
                    "bench".to_string(),
                    "update-key".to_string(),
                    vec![UpdateOp::Merge {
                        path: None,
                        value: json!({"extra": "field", "tags": ["x"]}),
                    }],
                )
                .await
            }
        });
    });
}

fn kv_contention_benchmark(c: &mut Criterion) {
    let rt = Runtime::new().expect("create tokio runtime");
    let mut group = c.benchmark_group("kv_store_contention");

    for concurrency in common::kv_contention_levels() {
        let kv = Arc::new(BuiltinKvStore::new(None));

        // Pre-populate key
        rt.block_on(kv.set(
            "bench".to_string(),
            "contended-key".to_string(),
            json!({"counter": 0}),
        ));

        group.throughput(Throughput::Elements(concurrency as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(concurrency),
            &concurrency,
            |b, &concurrency| {
                let kv = kv.clone();
                b.to_async(&rt).iter_custom(move |iters| {
                    let kv = kv.clone();
                    async move {
                        let start = Instant::now();
                        for _ in 0..iters {
                            let futures = (0..concurrency).map(|_| {
                                let kv = kv.clone();
                                async move {
                                    kv.update(
                                        "bench".to_string(),
                                        "contended-key".to_string(),
                                        vec![UpdateOp::Increment {
                                            path: FieldPath("counter".to_string()),
                                            by: 1,
                                        }],
                                    )
                                    .await;
                                }
                            });
                            join_all(futures).await;
                        }
                        start.elapsed()
                    }
                });
            },
        );
    }

    group.finish();
}

fn kv_list_benchmark(c: &mut Criterion) {
    let rt = Runtime::new().expect("create tokio runtime");
    let kv = Arc::new(BuiltinKvStore::new(None));

    // Pre-populate with 1000 keys across 10 indices
    rt.block_on(async {
        for idx in 0..10 {
            let index = format!("bench-index-{idx}");
            for key_idx in 0..100 {
                kv.set(index.clone(), format!("key-{key_idx}"), common::kv_value())
                    .await;
            }
        }
    });

    c.bench_function("kv_store/list_100_items", |b| {
        let kv = kv.clone();
        b.to_async(&rt).iter(|| {
            let kv = kv.clone();
            async move {
                let items = kv.list("bench-index-0".to_string()).await;
                assert_eq!(items.len(), 100);
            }
        });
    });

    c.bench_function("kv_store/list_keys_with_prefix", |b| {
        let kv = kv.clone();
        b.to_async(&rt).iter(|| {
            let kv = kv.clone();
            async move {
                let keys = kv.list_keys_with_prefix("bench-index-".to_string()).await;
                assert_eq!(keys.len(), 10);
            }
        });
    });

    c.bench_function("kv_store/list_groups", |b| {
        let kv = kv.clone();
        b.to_async(&rt).iter(|| {
            let kv = kv.clone();
            async move {
                let groups = kv.list_groups().await;
                assert_eq!(groups.len(), 10);
            }
        });
    });
}

criterion_group!(
    benches,
    kv_set_get_benchmark,
    kv_update_benchmark,
    kv_contention_benchmark,
    kv_list_benchmark,
);
criterion_main!(benches);
