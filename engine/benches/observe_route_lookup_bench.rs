//! What the routed hook's lookup costs every call once something is
//! observed: `route` for an observed function (`hit`) and for one nothing
//! observes (`miss`), with `n` observed functions in the routed view.
//!
//! `cargo bench -p iii --bench observe_route_lookup_bench`

use std::{hint::black_box, sync::Arc, time::Duration};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use iii::engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest};
use iii::function::FunctionResult;
use iii::observer::Deliver;
use iii::observer::{EngineObserver, ObserversConfig};
use iii::protocol::DEFAULT_NAMESPACE;
use iii::workers::observability::metrics::ensure_default_meter;

fn register(engine: &Engine, id: &str) {
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: id.to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(|_input| async { FunctionResult::Success(None) }),
    );
}

/// An engine whose routed view holds `n` observed functions.
fn engine_with(n: usize) -> Arc<Engine> {
    let engine = Arc::new(Engine::new());
    engine.observers.set_config(ObserversConfig {
        max_per_function: 1 << 20,
        ..ObserversConfig::default()
    });
    register(&engine, "bench::sink");
    register(&engine, "bench::unobserved");
    for i in 0..n {
        let function_id = format!("bench::fn-{i}");
        register(&engine, &function_id);
        engine
            .observers
            .observe_as_engine(EngineObserver {
                id: format!("obs-{i}"),
                function: (DEFAULT_NAMESPACE.to_string(), function_id),
                args: serde_json::json!({}),
                filters: None,
                target: (DEFAULT_NAMESPACE.to_string(), "bench::sink".to_string()),
                deliver: Deliver::Input,
                metadata: None,
            })
            .expect("observe");
    }
    assert!(
        engine
            .observers
            .route(DEFAULT_NAMESPACE, "bench::fn-0", None)
            .unwrap()
            .is_some(),
        "observed"
    );
    engine
}

fn route_lookup(c: &mut Criterion) {
    ensure_default_meter();
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let _guard = rt.enter();
    let mut group = c.benchmark_group("observe_route_lookup");
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));
    for n in [1, 10_000] {
        let engine = engine_with(n);
        let registry = &engine.observers;
        group.bench_with_input(BenchmarkId::new("hit", n), &n, |b, _| {
            b.iter(|| black_box(registry.route(DEFAULT_NAMESPACE, black_box("bench::fn-0"), None)))
        });
        group.bench_with_input(BenchmarkId::new("miss", n), &n, |b, _| {
            b.iter(|| {
                black_box(registry.route(DEFAULT_NAMESPACE, black_box("bench::unobserved"), None))
            })
        });
    }
    group.finish();
}

criterion_group!(benches, route_lookup);
criterion_main!(benches);
