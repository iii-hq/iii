mod common;

use std::time::{Duration, Instant};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use futures::future::join_all;
use iii::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::observability::metrics::ensure_default_meter,
};
use tokio::runtime::Runtime;

async fn run_parallel_batch(engine: Engine, concurrency: usize) {
    let payload = common::benchmark_payload();
    let futures = (0..concurrency).map(|_| engine.call("bench.echo", payload.clone()));
    let results = join_all(futures).await;

    for result in results {
        assert!(result.expect("engine call should succeed").is_some());
    }
}

fn concurrent_invocation_benchmark(c: &mut Criterion) {
    ensure_default_meter();

    let rt = Runtime::new().expect("create tokio runtime");
    let engine = Engine::new();

    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: "bench.echo".to_string(),
            description: Some("parallel invocation benchmark handler".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(|input| async move { FunctionResult::Success(Some(input)) }),
    );

    let mut group = c.benchmark_group("concurrent_invocation");
    group.measurement_time(Duration::from_secs(10));

    for concurrency in common::concurrency_levels() {
        group.throughput(Throughput::Elements(concurrency as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(concurrency),
            &concurrency,
            |b, &concurrency| {
                let engine = engine.clone();
                b.to_async(&rt).iter_custom(move |iters| {
                    let engine = engine.clone();
                    async move {
                        let start = Instant::now();
                        for _ in 0..iters {
                            run_parallel_batch(engine.clone(), concurrency).await;
                        }
                        start.elapsed()
                    }
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, concurrent_invocation_benchmark);
criterion_main!(benches);
