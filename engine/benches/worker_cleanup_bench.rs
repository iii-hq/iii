mod common;

use std::sync::Arc;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use iii::{
    engine::{Engine, EngineTrait, Handler, Outbound, RegisterFunctionRequest},
    function::{Function, FunctionResult, FunctionsRegistry},
    invocation::InvocationHandler,
    observer::{ObserveRequest, ObserverRegistry},
    protocol::{ObserveSource, ObserveTarget},
    services::ServicesRegistry,
    worker_connections::{WorkerConnection, WorkerConnectionRegistry},
    workers::observability::metrics::ensure_default_meter,
};
use tokio::{runtime::Runtime, sync::mpsc};

fn make_function(id: &str) -> Function {
    Function {
        handler: Arc::new(move |_invocation_id, input, _session, _metadata| {
            Box::pin(async move { FunctionResult::Success(Some(input)) })
        }),
        _function_id: id.to_string(),
        _description: Some("cleanup benchmark function".to_string()),
        request_format: None,
        response_format: None,
        metadata: None,
    }
}

/// Simulates a subset of Engine::cleanup_worker operations for registry teardown.
/// NOTE: This does not include external-function module cleanup, channel cleanup,
/// or the `engine::on-workers-changed` event.
async fn simulate_cleanup(
    worker: &WorkerConnection,
    functions: &FunctionsRegistry,
    service_registry: &ServicesRegistry,
    invocations: &InvocationHandler,
    observers: &Arc<ObserverRegistry>,
    worker_registry: &WorkerConnectionRegistry,
) {
    // Step 1: Read function_ids, remove each function and service
    let function_ids: Vec<String> = worker.function_ids.read().await.iter().cloned().collect();
    for function_id in &function_ids {
        functions.remove(iii::protocol::DEFAULT_NAMESPACE, function_id);
        service_registry
            .remove_function_from_services(iii::protocol::DEFAULT_NAMESPACE, function_id);
    }

    // Step 2: Read invocations, halt each.
    // Note: InvocationHandler has no public insert API, so halt_invocation
    // measures a DashMap miss lookup only (not the oneshot error-send path).
    let invocation_ids: Vec<uuid::Uuid> = worker.invocations.read().await.iter().cloned().collect();
    for invocation_id in &invocation_ids {
        invocations.halt_invocation(invocation_id);
    }

    // Step 3: Suspend the observers this worker created
    observers.connection_closed(worker.id, worker.reattach_token);

    // Step 4: Unregister the worker itself
    worker_registry.unregister_worker(&worker.id);
}

fn register_passthrough(engine: &Engine, function_id: &str) {
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

fn observe_request(idx: usize) -> ObserveRequest {
    ObserveRequest {
        id: format!("bench-observer-{idx}"),
        source: ObserveSource {
            function_id: "bench.source".to_string(),
            namespace: None,
            args: None,
            filters: None,
            capture: None,
            on: None,
            deliver: None,
        },
        target: ObserveTarget {
            function_id: "bench.target".to_string(),
            namespace: None,
        },
        metadata: None,
        max_fires: None,
        expires_in_ms: None,
        max_in_flight: None,
    }
}

fn worker_cleanup_benchmark(c: &mut Criterion) {
    ensure_default_meter();

    let rt = Runtime::new().expect("create tokio runtime");
    let mut group = c.benchmark_group("worker_cleanup");

    // Test with varying number of registered functions per worker
    for function_count in [10, 50, 200] {
        group.bench_with_input(
            BenchmarkId::new("functions", function_count),
            &function_count,
            |b, &function_count| {
                b.to_async(&rt).iter_batched(
                    || {
                        // Setup: create fresh registries and populate a worker
                        let functions = FunctionsRegistry::new();
                        let service_registry = ServicesRegistry::new();
                        let invocations = InvocationHandler::new();
                        let observers = Arc::new(ObserverRegistry::new());
                        let worker_registry = WorkerConnectionRegistry::new();

                        let (tx, _rx) = mpsc::channel::<Outbound>(1);
                        let worker = WorkerConnection::new(tx);

                        // Register functions belonging to this worker
                        for idx in 0..function_count {
                            let function_id = format!("bench.cleanup.{idx}");
                            functions.register_function(
                                function_id.clone(),
                                make_function(&function_id),
                            );
                            // Use block_on for the setup async calls
                            tokio::task::block_in_place(|| {
                                tokio::runtime::Handle::current()
                                    .block_on(worker.include_function_id(&function_id));
                            });
                            service_registry.register_service_from_function_id(
                                iii::protocol::DEFAULT_NAMESPACE,
                                &function_id,
                            );
                        }

                        worker_registry.register_worker(worker.clone());

                        (
                            worker,
                            functions,
                            service_registry,
                            invocations,
                            observers,
                            worker_registry,
                        )
                    },
                    |(
                        worker,
                        functions,
                        service_registry,
                        invocations,
                        observers,
                        worker_registry,
                    )| async move {
                        simulate_cleanup(
                            &worker,
                            &functions,
                            &service_registry,
                            &invocations,
                            &observers,
                            &worker_registry,
                        )
                        .await;
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }

    // Test with varying number of in-flight invocations
    for invocation_count in [10, 50, 200] {
        group.bench_with_input(
            BenchmarkId::new("invocations", invocation_count),
            &invocation_count,
            |b, &invocation_count| {
                b.to_async(&rt).iter_batched(
                    || {
                        let functions = FunctionsRegistry::new();
                        let service_registry = ServicesRegistry::new();
                        let invocations = InvocationHandler::new();
                        let observers = Arc::new(ObserverRegistry::new());
                        let worker_registry = WorkerConnectionRegistry::new();

                        let (tx, _rx) = mpsc::channel::<Outbound>(1);
                        let worker = WorkerConnection::new(tx);

                        // Add fake in-flight invocations to the worker
                        for _ in 0..invocation_count {
                            let invocation_id = uuid::Uuid::new_v4();
                            tokio::task::block_in_place(|| {
                                tokio::runtime::Handle::current()
                                    .block_on(worker.add_invocation(invocation_id));
                            });
                        }

                        worker_registry.register_worker(worker.clone());

                        (
                            worker,
                            functions,
                            service_registry,
                            invocations,
                            observers,
                            worker_registry,
                        )
                    },
                    |(
                        worker,
                        functions,
                        service_registry,
                        invocations,
                        observers,
                        worker_registry,
                    )| async move {
                        simulate_cleanup(
                            &worker,
                            &functions,
                            &service_registry,
                            &invocations,
                            &observers,
                            &worker_registry,
                        )
                        .await;
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }

    // Test with observers the worker created
    for observer_count in [10, 50, 200] {
        group.bench_with_input(
            BenchmarkId::new("observers", observer_count),
            &observer_count,
            |b, &observer_count| {
                b.to_async(&rt).iter_batched(
                    || {
                        let functions = FunctionsRegistry::new();
                        let service_registry = ServicesRegistry::new();
                        let invocations = InvocationHandler::new();
                        let worker_registry = WorkerConnectionRegistry::new();
                        let engine = Arc::new(Engine::new());
                        for function_id in ["bench.source", "bench.target"] {
                            register_passthrough(&engine, function_id);
                        }

                        let (tx, rx) = mpsc::channel::<Outbound>(1024);
                        let worker = WorkerConnection::new(tx);
                        engine.worker_registry.register_worker(worker.clone());

                        // Observers created by this worker
                        tokio::task::block_in_place(|| {
                            tokio::runtime::Handle::current().block_on(async {
                                for idx in 0..observer_count {
                                    engine
                                        .observers
                                        .observe(&engine, &worker, observe_request(idx))
                                        .await;
                                }
                            });
                        });

                        worker_registry.register_worker(worker.clone());

                        (
                            worker,
                            functions,
                            service_registry,
                            invocations,
                            engine,
                            worker_registry,
                            rx,
                        )
                    },
                    |(
                        worker,
                        functions,
                        service_registry,
                        invocations,
                        engine,
                        worker_registry,
                        rx,
                    )| async move {
                        simulate_cleanup(
                            &worker,
                            &functions,
                            &service_registry,
                            &invocations,
                            &engine.observers,
                            &worker_registry,
                        )
                        .await;
                        // The engine outlives a worker's cleanup, holding its
                        // suspended observers for the grace period: returned,
                        // it is dropped after the measurement.
                        (engine, rx)
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

criterion_group!(benches, worker_cleanup_benchmark);
criterion_main!(benches);
