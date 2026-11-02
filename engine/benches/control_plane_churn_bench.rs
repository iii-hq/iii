mod common;

use std::{sync::Arc, time::Duration};

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use iii::{
    engine::{Engine, EngineTrait, Handler, Outbound, RegisterFunctionRequest},
    function::{Function, FunctionResult, FunctionsRegistry},
    observer::EngineObserver,
    protocol::{DEFAULT_NAMESPACE, Deliver},
    worker_connections::{WorkerConnection, WorkerConnectionRegistry},
    workers::observability::metrics::ensure_default_meter,
};
use tokio::sync::mpsc;

/// An engine with the functions the observers churn between.
fn observed_engine() -> Arc<Engine> {
    let engine = Arc::new(Engine::new());
    for function_id in ["bench.source", "bench.target"] {
        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: function_id.to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(
                |input: serde_json::Value| async move { FunctionResult::Success(Some(input)) },
            ),
        );
    }
    engine
}

fn make_function(id: &str) -> Function {
    Function {
        handler: std::sync::Arc::new(move |_invocation_id, input, _session, _metadata| {
            Box::pin(async move { FunctionResult::Success(Some(input)) })
        }),
        _function_id: id.to_string(),
        _description: Some("control plane churn benchmark".to_string()),
        request_format: None,
        response_format: None,
        metadata: None,
    }
}

fn control_plane_churn_benchmark(c: &mut Criterion) {
    ensure_default_meter();

    let rt = tokio::runtime::Runtime::new().expect("create tokio runtime");
    let mut group = c.benchmark_group("control_plane_churn");
    group.measurement_time(Duration::from_secs(10));

    for size in common::churn_batch_sizes() {
        group.bench_with_input(
            BenchmarkId::new("functions_register_remove", size),
            &size,
            |b, &size| {
                b.iter_batched(
                    FunctionsRegistry::new,
                    |registry| {
                        for idx in 0..size {
                            let function_id = format!("bench.function.{idx}");
                            registry.register_function(
                                function_id.clone(),
                                make_function(&function_id),
                            );
                        }
                        for idx in 0..size {
                            registry.remove(
                                iii::protocol::DEFAULT_NAMESPACE,
                                &format!("bench.function.{idx}"),
                            );
                        }
                    },
                    BatchSize::LargeInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("workers_register_unregister", size),
            &size,
            |b, &size| {
                b.iter_batched(
                    WorkerConnectionRegistry::new,
                    |registry| {
                        let mut ids = Vec::with_capacity(size);
                        let mut _receivers = Vec::with_capacity(size);
                        for _ in 0..size {
                            let (tx, rx) = mpsc::channel::<Outbound>(1);
                            let worker = WorkerConnection::new(tx);
                            ids.push(worker.id);
                            _receivers.push(rx);
                            registry.register_worker(worker);
                        }
                        for id in ids {
                            registry.unregister_worker(&id);
                        }
                    },
                    BatchSize::LargeInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("observers_add_remove", size),
            &size,
            |b, &size| {
                b.to_async(&rt).iter_batched(
                    observed_engine,
                    |engine| async move {
                        let function = (DEFAULT_NAMESPACE.to_string(), "bench.source".to_string());
                        let target = (DEFAULT_NAMESPACE.to_string(), "bench.target".to_string());
                        for idx in 0..size {
                            engine
                                .observers
                                .observe_as_engine(EngineObserver {
                                    id: format!("bench-observer-{idx}"),
                                    function: function.clone(),
                                    args: serde_json::json!({}),
                                    filters: None,
                                    target: target.clone(),
                                    deliver: Deliver::Input,
                                    metadata: None,
                                })
                                .expect("observe");
                        }
                        for idx in 0..size {
                            engine
                                .observers
                                .unobserve_as_engine(&format!("bench-observer-{idx}"));
                        }
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }

    group.finish();
}

criterion_group!(benches, control_plane_churn_benchmark);
criterion_main!(benches);
