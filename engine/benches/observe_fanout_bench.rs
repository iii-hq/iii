//! What one call of an engine-hosted source costs when `n` observers take it.
//!
//! The source and every observer's target run in-process, so the measurement
//! is the engine's own work: building the event, evaluating the observers and
//! running one activation per observer. An iteration ends when every target
//! has run.
//!
//! - `observe_fanout/<n>`: one event delivered to `n` engine-owned observers
//!   without filters, each with its own target.
//!
//! `cargo bench -p iii --bench observe_fanout_bench`

mod common;

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use iii::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    observer::{EngineObserver, events::register_event_source},
    protocol::{DEFAULT_NAMESPACE, Deliver},
    workers::observability::metrics::ensure_default_meter,
};
use serde_json::json;
use tokio::{runtime::Runtime, sync::Notify};

const SOURCE: &str = "bench::on-event";

async fn wait_for_completion(counter: Arc<AtomicUsize>, notify: Arc<Notify>, expected: usize) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while counter.load(Ordering::SeqCst) < expected {
            notify.notified().await;
        }
    })
    .await
    .expect("fanout completion timed out");
}

async fn build_fanout_engine(fanout: usize) -> (Arc<Engine>, Arc<AtomicUsize>, Arc<Notify>) {
    let engine = Arc::new(Engine::new());
    let completed = Arc::new(AtomicUsize::new(0));
    let notify = Arc::new(Notify::new());
    register_event_source(&engine, SOURCE, "criterion fanout source", json!({}));

    for idx in 0..fanout {
        let function_id = format!("bench::fanout-{idx}");
        let completed_ref = completed.clone();
        let notify_ref = notify.clone();
        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: function_id.clone(),
                description: Some("fanout target".to_string()),
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
            .observers
            .observe_as_engine(EngineObserver {
                id: format!("bench-observer-{idx}"),
                function: (DEFAULT_NAMESPACE.to_string(), SOURCE.to_string()),
                args: json!({}),
                filters: None,
                target: (DEFAULT_NAMESPACE.to_string(), function_id),
                deliver: Deliver::Input,
                metadata: None,
            })
            .expect("observe the source");
    }

    (engine, completed, notify)
}

fn observe_fanout_benchmark(c: &mut Criterion) {
    ensure_default_meter();

    let rt = Runtime::new().expect("create tokio runtime");
    let mut group = c.benchmark_group("observe_fanout");
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
                                .observers
                                .emit(DEFAULT_NAMESPACE, SOURCE, || Some(common::event_payload()));
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

criterion_group!(benches, observe_fanout_benchmark);
criterion_main!(benches);
