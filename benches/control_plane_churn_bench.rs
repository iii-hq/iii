mod common;

use std::{pin::Pin, time::Duration};

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use futures::Future;
use iii::{
    engine::Outbound,
    function::{Function, FunctionResult, FunctionsRegistry},
    modules::observability::metrics::ensure_default_meter,
    trigger::{Trigger, TriggerRegistrator, TriggerRegistry, TriggerType},
    workers::{Worker, WorkerRegistry},
};
use tokio::sync::mpsc;

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
        handler: std::sync::Arc::new(move |_invocation_id, input| {
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
                            registry.remove(&format!("bench.function.{idx}"));
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
                    WorkerRegistry::new,
                    |registry| {
                        let mut ids = Vec::with_capacity(size);
                        for _ in 0..size {
                            let (tx, _rx) = mpsc::channel::<Outbound>(1);
                            let worker = Worker::new(tx);
                            ids.push(worker.id);
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
            BenchmarkId::new("triggers_register_unregister", size),
            &size,
            |b, &size| {
                b.to_async(&rt).iter_batched(
                    TriggerRegistry::new,
                    |registry| async move {
                        registry
                            .register_trigger_type(TriggerType {
                                id: "bench.trigger".to_string(),
                                _description: "control plane benchmark".to_string(),
                                registrator: Box::new(NoopRegistrator),
                                worker_id: None,
                            })
                            .await
                            .expect("register trigger type");

                        for idx in 0..size {
                            registry
                                .register_trigger(Trigger {
                                    id: format!("bench-trigger-{idx}"),
                                    trigger_type: "bench.trigger".to_string(),
                                    function_id: format!("bench.function.{idx}"),
                                    config: serde_json::json!({}),
                                    worker_id: None,
                                })
                                .await
                                .expect("register trigger");
                        }

                        for idx in 0..size {
                            registry
                                .unregister_trigger(
                                    format!("bench-trigger-{idx}"),
                                    Some("bench.trigger".to_string()),
                                )
                                .await
                                .expect("unregister trigger");
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
