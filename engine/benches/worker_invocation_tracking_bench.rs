mod common;

use std::{sync::Arc, time::Instant};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use futures::future::join_all;
use iii::{
    engine::Outbound, modules::observability::metrics::ensure_default_meter, workers::Worker,
};
use tokio::{runtime::Runtime, sync::mpsc};
use uuid::Uuid;

fn add_remove_sequential_benchmark(c: &mut Criterion) {
    ensure_default_meter();

    let rt = Runtime::new().expect("create tokio runtime");
    let (tx, _rx) = mpsc::channel::<Outbound>(64);
    let worker = Worker::new(tx);

    c.bench_function("worker_invocation_tracking/add_remove_sequential", |b| {
        b.to_async(&rt).iter(|| async {
            let id = Uuid::new_v4();
            worker.add_invocation(id).await;
            worker.remove_invocation(&id).await;
        });
    });
}

fn add_remove_concurrent_benchmark(c: &mut Criterion) {
    ensure_default_meter();

    let rt = Runtime::new().expect("create tokio runtime");
    let mut group = c.benchmark_group("worker_invocation_tracking_concurrent");

    for concurrency in common::invocation_tracking_levels() {
        let (tx, _rx) = mpsc::channel::<Outbound>(64);
        let worker = Arc::new(Worker::new(tx));

        group.throughput(Throughput::Elements(concurrency as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(concurrency),
            &concurrency,
            |b, &concurrency| {
                let worker = worker.clone();
                b.to_async(&rt).iter_custom(move |iters| {
                    let worker = worker.clone();
                    async move {
                        let start = Instant::now();
                        for _ in 0..iters {
                            // Concurrent add
                            let ids: Vec<Uuid> = (0..concurrency).map(|_| Uuid::new_v4()).collect();
                            let add_futures = ids.iter().map(|id| {
                                let worker = worker.clone();
                                let id = *id;
                                async move {
                                    worker.add_invocation(id).await;
                                }
                            });
                            join_all(add_futures).await;

                            // Concurrent remove
                            let remove_futures = ids.iter().map(|id| {
                                let worker = worker.clone();
                                async move {
                                    worker.remove_invocation(id).await;
                                }
                            });
                            join_all(remove_futures).await;
                        }
                        start.elapsed()
                    }
                });
            },
        );
    }

    group.finish();
}

fn add_under_load_benchmark(c: &mut Criterion) {
    ensure_default_meter();

    let rt = Runtime::new().expect("create tokio runtime");
    let (tx, _rx) = mpsc::channel::<Outbound>(64);
    let worker = Worker::new(tx);

    // Pre-fill the invocations set to simulate a busy worker
    let pre_fill_count = 200;
    rt.block_on(async {
        for _ in 0..pre_fill_count {
            worker.add_invocation(Uuid::new_v4()).await;
        }
    });

    c.bench_function("worker_invocation_tracking/add_with_200_existing", |b| {
        b.to_async(&rt).iter(|| async {
            let id = Uuid::new_v4();
            worker.add_invocation(id).await;
            worker.remove_invocation(&id).await;
        });
    });
}

criterion_group!(
    benches,
    add_remove_sequential_benchmark,
    add_remove_concurrent_benchmark,
    add_under_load_benchmark,
);
criterion_main!(benches);
