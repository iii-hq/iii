mod common;

use std::{
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use futures::Future;
use iii::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::observability::metrics::ensure_default_meter,
    trigger::{Trigger, TriggerRegistrator, TriggerType},
};
use tokio::{runtime::Runtime, sync::Notify};

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

async fn wait_for_completion(counter: Arc<AtomicUsize>, notify: Arc<Notify>, expected: usize) {
    while counter.load(Ordering::SeqCst) < expected {
        notify.notified().await;
    }
}

async fn build_fanout_engine(fanout: usize) -> (Engine, Arc<AtomicUsize>, Arc<Notify>) {
    let engine = Engine::new();
    let completed = Arc::new(AtomicUsize::new(0));
    let notify = Arc::new(Notify::new());

    engine
        .register_trigger_type(TriggerType {
            id: "bench.trigger".to_string(),
            _description: "criterion fanout trigger".to_string(),
            registrator: Box::new(NoopRegistrator),
            worker_id: None,
        })
        .await;

    for idx in 0..fanout {
        let function_id = format!("bench.fanout.{idx}");
        let completed_ref = completed.clone();
        let notify_ref = notify.clone();

        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: function_id.clone(),
                description: Some("fanout handler".to_string()),
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(move |_input| {
                let completed_ref = completed_ref.clone();
                let notify_ref = notify_ref.clone();
                async move {
                    completed_ref.fetch_add(1, Ordering::SeqCst);
                    notify_ref.notify_one();
                    FunctionResult::Success(None)
                }
            }),
        );

        engine
            .trigger_registry
            .register_trigger(Trigger {
                id: format!("bench-trigger-{idx}"),
                trigger_type: "bench.trigger".to_string(),
                function_id,
                config: serde_json::json!({}),
                worker_id: None,
            })
            .await
            .expect("register trigger");
    }

    (engine, completed, notify)
}

fn trigger_fanout_benchmark(c: &mut Criterion) {
    ensure_default_meter();

    let rt = Runtime::new().expect("create tokio runtime");
    let mut group = c.benchmark_group("trigger_fanout");
    group.measurement_time(Duration::from_secs(10));

    for fanout in common::fanout_levels() {
        let (engine, completed, notify) = rt.block_on(build_fanout_engine(fanout));

        group.throughput(Throughput::Elements(fanout as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(fanout),
            &fanout,
            |b, &fanout| {
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
                            completed.store(0, Ordering::SeqCst);
                            engine
                                .fire_triggers("bench.trigger", common::trigger_payload())
                                .await;
                            wait_for_completion(completed.clone(), notify.clone(), fanout).await;
                        }
                        start.elapsed()
                    }
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, trigger_fanout_benchmark);
criterion_main!(benches);
