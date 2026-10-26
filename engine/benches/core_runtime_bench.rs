mod common;

use std::{hint::black_box, time::Duration};

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use iii::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    workers::observability::metrics::{MetricsAccumulator, ensure_default_meter},
};

fn core_runtime_benchmark(c: &mut Criterion) {
    ensure_default_meter();

    let rt = tokio::runtime::Runtime::new().expect("create tokio runtime");
    let engine = Engine::new();

    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: "bench.echo".to_string(),
            description: Some("criterion benchmark echo handler".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(|input| async move { FunctionResult::Success(Some(input)) }),
    );

    let payload = common::benchmark_payload();
    c.bench_function("core_runtime/engine_call_registered_handler", |b| {
        let engine = engine.clone();
        let payload = payload.clone();
        b.to_async(&rt).iter(|| {
            let engine = engine.clone();
            let payload = payload.clone();
            async move {
                let response = engine
                    .call("bench.echo", payload)
                    .await
                    .expect("engine call should succeed");
                assert!(response.is_some());
            }
        });
    });
}

fn metrics_accumulator_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("metrics_accumulator");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(100));
    group.measurement_time(Duration::from_millis(200));

    let accumulator = MetricsAccumulator::default();
    let function_id = "orders::get";
    accumulator.increment_function(function_id);
    group.throughput(Throughput::Elements(1));
    group.bench_function("existing_function", |b| {
        b.iter(|| accumulator.increment_function(black_box(function_id)));
    });

    let function_ids: Vec<_> = (0..128).map(|i| format!("orders::get-{i}")).collect();
    group.throughput(Throughput::Elements(function_ids.len() as u64));
    group.bench_function("first_invocations", |b| {
        b.iter_batched_ref(
            MetricsAccumulator::default,
            |accumulator| {
                for function_id in &function_ids {
                    accumulator.increment_function(black_box(function_id));
                }
            },
            BatchSize::PerIteration,
        );
    });
    group.finish();
}

criterion_group!(
    benches,
    core_runtime_benchmark,
    metrics_accumulator_benchmark
);
criterion_main!(benches);
