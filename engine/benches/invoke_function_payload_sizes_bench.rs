mod common;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use iii::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::observability::metrics::ensure_default_meter,
};
use tokio::runtime::Runtime;

fn invoke_function_payload_sizes_benchmark(c: &mut Criterion) {
    ensure_default_meter();

    let rt = Runtime::new().expect("create tokio runtime");
    let engine = Engine::new();

    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: "bench.echo".to_string(),
            description: Some("payload-size benchmark echo handler".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(|input| async move { FunctionResult::Success(Some(input)) }),
    );

    let mut group = c.benchmark_group("invoke_function_payload_sizes");

    for (label, size) in common::payload_sizes() {
        let payload = common::sized_payload(size);
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &payload,
            |b, payload| {
                b.to_async(&rt).iter(|| async {
                    let response = engine
                        .call("bench.echo", payload.clone())
                        .await
                        .expect("engine call should succeed");

                    assert!(response.is_some());
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, invoke_function_payload_sizes_benchmark);
criterion_main!(benches);
