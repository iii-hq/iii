mod common;

use std::time::Instant;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use iii::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::observability::metrics::ensure_default_meter,
};
use tokio::runtime::Runtime;

/// Simulates PubSub LocalAdapter.publish: spawn one engine.call per subscriber,
/// wait for all to complete. Errors are surfaced via JoinHandle assertions.
async fn simulate_publish(engine: &Engine, subscriber_count: usize) {
    let payload = common::trigger_payload();
    let mut handles = Vec::with_capacity(subscriber_count);

    for idx in 0..subscriber_count {
        let engine = engine.clone();
        let data = payload.clone();

        handles.push(tokio::spawn(async move {
            let function_id = format!("bench.subscriber.{idx}");
            engine
                .call(&function_id, data)
                .await
                .expect("subscriber call should succeed");
        }));
    }

    for handle in handles {
        handle.await.expect("subscriber task panicked");
    }
}

fn pubsub_fanout_benchmark(c: &mut Criterion) {
    ensure_default_meter();

    let rt = Runtime::new().expect("create tokio runtime");
    let mut group = c.benchmark_group("pubsub_fanout");

    for subscriber_count in common::pubsub_subscriber_counts() {
        let engine = Engine::new();

        // Register N subscriber functions
        for idx in 0..subscriber_count {
            engine.register_function_handler(
                RegisterFunctionRequest {
                    function_id: format!("bench.subscriber.{idx}"),
                    description: Some("pubsub subscriber handler".to_string()),
                    request_format: None,
                    response_format: None,
                    metadata: None,
                },
                Handler::new(move |_input| async move { FunctionResult::Success(None) }),
            );
        }

        group.throughput(Throughput::Elements(subscriber_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(subscriber_count),
            &subscriber_count,
            |b, &subscriber_count| {
                let engine = engine.clone();

                b.to_async(&rt).iter_custom(move |iters| {
                    let engine = engine.clone();

                    async move {
                        let start = Instant::now();
                        for _ in 0..iters {
                            simulate_publish(&engine, subscriber_count).await;
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
