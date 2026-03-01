mod common;

use std::{pin::Pin, sync::Arc};

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use futures::Future;
use iii::{
    engine::Outbound,
    function::{Function, FunctionResult, FunctionsRegistry},
    invocation::InvocationHandler,
    modules::observability::metrics::ensure_default_meter,
    services::ServicesRegistry,
    trigger::{Trigger, TriggerRegistrator, TriggerRegistry, TriggerType},
    workers::{Worker, WorkerRegistry},
};
use tokio::{runtime::Runtime, sync::mpsc};

#[derive(Clone)]
struct NoopRegistrator;

impl TriggerRegistrator for NoopRegistrator {
    fn register_trigger(
        &self,
        _trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }

    fn unregister_trigger(
        &self,
        _trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }
}

fn make_function(id: &str) -> Function {
    Function {
        handler: Arc::new(move |_invocation_id, input| {
            Box::pin(async move { FunctionResult::Success(Some(input)) })
        }),
        _function_id: id.to_string(),
        _description: Some("cleanup benchmark function".to_string()),
        request_format: None,
        response_format: None,
        metadata: None,
    }
}

/// Simulates the exact operations performed by Engine::cleanup_worker.
async fn simulate_cleanup(
    worker: &Worker,
    functions: &FunctionsRegistry,
    service_registry: &ServicesRegistry,
    invocations: &InvocationHandler,
    trigger_registry: &TriggerRegistry,
    worker_registry: &WorkerRegistry,
) {
    // Step 1: Read function_ids, remove each function and service
    let function_ids: Vec<String> = worker.function_ids.read().await.iter().cloned().collect();
    for function_id in &function_ids {
        functions.remove(function_id);
        service_registry.remove_function_from_services(function_id);
    }

    // Step 2: Read invocations, halt each
    let invocation_ids: Vec<uuid::Uuid> = worker.invocations.read().await.iter().cloned().collect();
    for invocation_id in &invocation_ids {
        invocations.halt_invocation(invocation_id);
    }

    // Step 3: Unregister triggers belonging to this worker
    trigger_registry.unregister_worker(&worker.id).await;

    // Step 4: Unregister the worker itself
    worker_registry.unregister_worker(&worker.id);
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
                        let trigger_registry = TriggerRegistry::new();
                        let worker_registry = WorkerRegistry::new();

                        let (tx, _rx) = mpsc::channel::<Outbound>(1);
                        let worker = Worker::new(tx);

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
                            service_registry.register_service_from_function_id(&function_id);
                        }

                        worker_registry.register_worker(worker.clone());

                        (
                            worker,
                            functions,
                            service_registry,
                            invocations,
                            trigger_registry,
                            worker_registry,
                        )
                    },
                    |(
                        worker,
                        functions,
                        service_registry,
                        invocations,
                        trigger_registry,
                        worker_registry,
                    )| async move {
                        simulate_cleanup(
                            &worker,
                            &functions,
                            &service_registry,
                            &invocations,
                            &trigger_registry,
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
                        let trigger_registry = TriggerRegistry::new();
                        let worker_registry = WorkerRegistry::new();

                        let (tx, _rx) = mpsc::channel::<Outbound>(1);
                        let worker = Worker::new(tx);

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
                            trigger_registry,
                            worker_registry,
                        )
                    },
                    |(
                        worker,
                        functions,
                        service_registry,
                        invocations,
                        trigger_registry,
                        worker_registry,
                    )| async move {
                        simulate_cleanup(
                            &worker,
                            &functions,
                            &service_registry,
                            &invocations,
                            &trigger_registry,
                            &worker_registry,
                        )
                        .await;
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }

    // Test with triggers belonging to the worker
    for trigger_count in [10, 50, 200] {
        group.bench_with_input(
            BenchmarkId::new("triggers", trigger_count),
            &trigger_count,
            |b, &trigger_count| {
                b.to_async(&rt).iter_batched(
                    || {
                        let functions = FunctionsRegistry::new();
                        let service_registry = ServicesRegistry::new();
                        let invocations = InvocationHandler::new();
                        let trigger_registry = TriggerRegistry::new();
                        let worker_registry = WorkerRegistry::new();

                        let (tx, _rx) = mpsc::channel::<Outbound>(1);
                        let worker = Worker::new(tx);

                        // Register trigger type and triggers owned by this worker
                        tokio::task::block_in_place(|| {
                            tokio::runtime::Handle::current().block_on(async {
                                let _ = trigger_registry
                                    .register_trigger_type(TriggerType {
                                        id: "bench.cleanup".to_string(),
                                        _description: "cleanup benchmark trigger".to_string(),
                                        registrator: Box::new(NoopRegistrator),
                                        worker_id: Some(worker.id),
                                    })
                                    .await;

                                for idx in 0..trigger_count {
                                    let _ = trigger_registry
                                        .register_trigger(Trigger {
                                            id: format!("bench-cleanup-{idx}"),
                                            trigger_type: "bench.cleanup".to_string(),
                                            function_id: format!("bench.cleanup.{idx}"),
                                            config: serde_json::json!({}),
                                            worker_id: Some(worker.id),
                                        })
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
                            trigger_registry,
                            worker_registry,
                        )
                    },
                    |(
                        worker,
                        functions,
                        service_registry,
                        invocations,
                        trigger_registry,
                        worker_registry,
                    )| async move {
                        simulate_cleanup(
                            &worker,
                            &functions,
                            &service_registry,
                            &invocations,
                            &trigger_registry,
                            &worker_registry,
                        )
                        .await;
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
