mod common;

use criterion::{Criterion, criterion_group, criterion_main};
use iii::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::observability::metrics::ensure_default_meter,
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

    c.bench_function("core_runtime/engine_call_registered_handler", |b| {
        b.to_async(&rt).iter(|| async {
            let payload = common::benchmark_payload();
            let response = engine
                .call("bench.echo", payload)
                .await
                .expect("engine call should succeed");
            assert!(response.is_some());
        });
    });
}

criterion_group!(benches, core_runtime_benchmark);
criterion_main!(benches);
