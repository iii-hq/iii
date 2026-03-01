mod common;

use std::{sync::Arc, time::Instant};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use futures::future::join_all;
use iii::builtins::{
    kv::BuiltinKvStore,
    pubsub_lite::BuiltInPubSubLite,
    queue::{BuiltinQueue, QueueConfig},
    queue_kv::QueueKvStore,
};
use tokio::runtime::Runtime;

fn build_queue() -> Arc<BuiltinQueue> {
    let base_kv = Arc::new(BuiltinKvStore::new(None));
    let kv_store = Arc::new(QueueKvStore::new(base_kv, None));
    let pubsub = Arc::new(BuiltInPubSubLite::new(None));
    Arc::new(BuiltinQueue::new(kv_store, pubsub, QueueConfig::default()))
}

fn queue_push_benchmark(c: &mut Criterion) {
    let rt = Runtime::new().expect("create tokio runtime");
    let queue = build_queue();
    let payload = common::benchmark_payload();

    c.bench_function("queue_enqueue/push_single", |b| {
        let queue = queue.clone();
        let payload = payload.clone();
        b.to_async(&rt).iter(|| {
            let queue = queue.clone();
            let payload = payload.clone();
            async move {
                queue.push("bench-topic", payload, None, None).await;
            }
        });
    });
}

fn queue_concurrent_push_benchmark(c: &mut Criterion) {
    let rt = Runtime::new().expect("create tokio runtime");
    let mut group = c.benchmark_group("queue_enqueue_concurrent");

    for producer_count in common::queue_producer_counts() {
        let queue = build_queue();
        let payload = common::benchmark_payload();

        group.throughput(Throughput::Elements(producer_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(producer_count),
            &producer_count,
            |b, &producer_count| {
                let queue = queue.clone();
                let payload = payload.clone();
                b.to_async(&rt).iter_custom(move |iters| {
                    let queue = queue.clone();
                    let payload = payload.clone();
                    async move {
                        let start = Instant::now();
                        for _ in 0..iters {
                            let futures = (0..producer_count).map(|_| {
                                let queue = queue.clone();
                                let payload = payload.clone();
                                async move {
                                    queue.push("bench-topic", payload, None, None).await;
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

fn queue_push_payload_sizes_benchmark(c: &mut Criterion) {
    let rt = Runtime::new().expect("create tokio runtime");
    let queue = build_queue();
    let mut group = c.benchmark_group("queue_enqueue_payload_sizes");

    for (label, size) in common::payload_sizes() {
        let payload = common::sized_payload(size);
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &payload,
            |b, payload| {
                let queue = queue.clone();
                let payload = payload.clone();
                b.to_async(&rt).iter(|| {
                    let queue = queue.clone();
                    let payload = payload.clone();
                    async move {
                        queue.push("bench-topic", payload, None, None).await;
                    }
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    queue_push_benchmark,
    queue_concurrent_push_benchmark,
    queue_push_payload_sizes_benchmark,
);
criterion_main!(benches);
