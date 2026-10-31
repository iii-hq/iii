//! What observing a function costs the engine per call, for the calls the
//! engine evaluates itself (engine-hosted functions, hosts without
//! `observe.v2`).
//!
//! The observed function and the observers' target both run in-process, so
//! the measurement is the engine's own work: the call, the routed-path hook
//! that matches the observers' filters (through the function's filter index),
//! and the activations it starts.
//! An iteration ends when every activation it started has reached the target,
//! so fan-out is paid for inside the measured time, not left as a backlog;
//! calls go in batches that wait for the evaluator, so none is dropped.
//!
//! - `observe_dispatch/default/<n>`: `n` observers without filters on the
//!   function: every call activates all of them (0 is the unobserved baseline).
//! - `observe_dispatch/filtered/<n>`: `n` observers each filtering on its own
//!   value of `input.k`: every call matches exactly one. The index finds it
//!   with one lookup, so the cost does not grow with `n`.
//!
//! The observers are created by a WebSocket worker, as in the observe e2e
//! tests (their support module is shared).
//!
//! `cargo bench -p iii --bench observe_dispatch_bench`

#[path = "../tests/observe_support/mod.rs"]
mod observe_support;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use iii::engine::EngineTrait;
use iii::observer::ObserversConfig;
use serde_json::{Value, json};
use tokio::runtime::Runtime;

use observe_support::{TestEngine, connect, in_process, spawn_engine};

const OBSERVED: &str = "bench::observed";
const TARGET: &str = "bench::target";

struct Bench {
    t: TestEngine,
    delivered: Arc<AtomicU64>,
    // Keeps the observing connection, and with it the observers, alive.
    _observer: observe_support::Client,
}

async fn setup(observers: usize, filtered: bool) -> Bench {
    let t = spawn_engine(ObserversConfig {
        // Room for every activation of a batch: the bench measures work, not
        // the drop policy.
        default_max_in_flight: 4096,
        max_queue_per_observer: 65_536,
        // Every observer comes from one connection.
        max_per_connection: 8192,
        max_per_function: 8192,
        ..ObserversConfig::default()
    })
    .await;
    in_process(&t.engine, OBSERVED, |input| input);
    let delivered = Arc::new(AtomicU64::new(0));
    let counter = delivered.clone();
    in_process(&t.engine, TARGET, move |_| {
        counter.fetch_add(1, Ordering::Relaxed);
        Value::Null
    });
    let mut observer = connect(&t, "observer", true).await;
    for i in 0..observers {
        let source = if filtered {
            json!({ "function_id": OBSERVED, "filters": { "input.k": { "$eq": i } } })
        } else {
            json!({ "function_id": OBSERVED })
        };
        observer
            .observe_active(json!({
                "id": format!("obs-{i}"),
                "source": source,
                "target": { "function_id": TARGET },
            }))
            .await;
    }
    Bench {
        t,
        delivered,
        _observer: observer,
    }
}

/// Calls between two waits for the evaluator: the routed hook hands each
/// observed call to one evaluator task through a bounded queue (64k), and a
/// tight in-process loop would otherwise overflow it and count drops.
const BATCH: u64 = 1024;

/// `iters` calls in batches, each waited on until the evaluator has caught up,
/// then until `per_call * iters` activations arrived.
async fn run(b: &Bench, iters: u64, per_call: u64, keys: u64) -> Duration {
    let observed = per_call > 0;
    let evaluated = b.t.engine.observers.evaluated();
    let expected = b.delivered.load(Ordering::Relaxed) + per_call * iters;
    let deadline = Instant::now() + Duration::from_secs(60);
    let start = Instant::now();
    for i in 0..iters {
        let k = if keys > 0 { i % keys } else { 0 };
        let out = b.t.engine.call(OBSERVED, json!({ "k": k })).await;
        assert!(out.is_ok(), "call failed");
        if observed && (i + 1) % BATCH == 0 {
            while b.t.engine.observers.evaluated() < evaluated + i + 1 {
                assert!(Instant::now() < deadline, "the evaluator never caught up");
                tokio::task::yield_now().await;
            }
        }
    }
    while b.delivered.load(Ordering::Relaxed) < expected {
        assert!(
            Instant::now() < deadline,
            "activations did not all arrive: {} of {expected}, {} calls evaluated",
            b.delivered.load(Ordering::Relaxed),
            b.t.engine.observers.evaluated() - evaluated,
        );
        tokio::task::yield_now().await;
    }
    start.elapsed()
}

fn observe_dispatch(c: &mut Criterion) {
    let rt = Runtime::new().expect("tokio runtime");
    let mut group = c.benchmark_group("observe_dispatch");
    group.measurement_time(Duration::from_secs(8));
    group.throughput(Throughput::Elements(1));

    for n in [0usize, 1, 8, 64] {
        let bench = rt.block_on(setup(n, false));
        group.bench_with_input(BenchmarkId::new("default", n), &n, |b, &n| {
            b.to_async(&rt)
                .iter_custom(|iters| run(&bench, iters, n as u64, 0));
        });
    }
    for n in [16usize, 256, 1024, 4000] {
        let bench = rt.block_on(setup(n, true));
        group.bench_with_input(BenchmarkId::new("filtered", n), &n, |b, &n| {
            b.to_async(&rt)
                .iter_custom(|iters| run(&bench, iters, 1, n as u64));
        });
    }
    group.finish();
}

criterion_group!(benches, observe_dispatch);
criterion_main!(benches);
