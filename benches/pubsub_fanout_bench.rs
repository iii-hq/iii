mod common;

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use iii::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::observability::metrics::ensure_default_meter,
};
use tokio::{runtime::Runtime, sync::Notify};

/// Simulates PubSub LocalAdapter.publish: spawn one engine.call per subscriber,
/// wait for all to complete.
async fn simulate_publish(
    engine: &Engine,
    subscriber_count: usize,
    completed: &AtomicUsize,
    notify: &Notify,
) {
    completed.store(0, Ordering::SeqCst);

    let payload = common::trigger_payload();

    for idx in 0..subscriber_count {
        let engine = engine.clone();
        let data = payload.clone();
        let completed = completed as *const AtomicUsize as usize;
        let notify = notify as *const Notify as usize;

        tokio::spawn(async move {
            let function_id = format!("bench.subscriber.{idx}");
            let _ = engine.call(&function_id, data).await;

            // SAFETY: pointers are valid for the duration of the benchmark iteration
            let completed = unsafe { &*(completed as *const AtomicUsize) };
            let notify = unsafe { &*(notify as *const Notify) };
            completed.fetch_add(1, Ordering::SeqCst);
            notify.notify_one();
        });
    }

    // Wait for all subscribers to complete
    while completed.load(Ordering::SeqCst) < subscriber_count {
        notify.notified().await;
    }
}

fn pubsub_fanout_benchmark(c: &mut Criterion) {
    ensure_default_meter();

    let rt = Runtime::new().expect("create tokio runtime");
    let mut group = c.benchmark_group("pubsub_fanout");

    for subscriber_count in common::pubsub_subscriber_counts() {
        let engine = Engine::new();
        let completed = Arc::new(AtomicUsize::new(0));
        let notify = Arc::new(Notify::new());

        // Register N subscriber functions
        for idx in 0..subscriber_count {
            let completed_ref = completed.clone();
            let notify_ref = notify.clone();

            engine.register_function_handler(
                RegisterFunctionRequest {
                    function_id: format!("bench.subscriber.{idx}"),
                    description: Some("pubsub subscriber handler".to_string()),
                    request_format: None,
                    response_format: None,
                    metadata: None,
                },
                Handler::new(move |_input| {
                    let _completed = completed_ref.clone();
                    let _notify = notify_ref.clone();
                    async move { FunctionResult::Success(None) }
                }),
            );
        }

        group.throughput(Throughput::Elements(subscriber_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(subscriber_count),
            &subscriber_count,
            |b, &subscriber_count| {
                let engine = engine.clone();
                let completed = completed.clone();
                let notify = notify.clone();

                b.to_async(&rt).iter_custom(move |iters| {
                    let engine = engine.clone();
                    let completed = completed.clone();
                    let notify = notify.clone();

                    async move {
                        let start = Instant::now();
                        for _ in 0..iters {
                            simulate_publish(&engine, subscriber_count, &completed, &notify).await;
                        }
                        start.elapsed()
                    }
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, pubsub_fanout_benchmark);
criterion_main!(benches);
