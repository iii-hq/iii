//! What observing a function costs a routed call whose host delivers it
//! (v2 §8.2): the host lists the observers its delivery matched on its
//! answer (`invocationresult.observed`), so an observed call takes the same
//! frames as an unobserved one.
//!
//! A WebSocket worker that declared `observe.v2` hosts the function and
//! answers every call at once, listing every observer of the function. The
//! observers' target runs in-process. An iteration is one call, from the
//! engine to the host and back, and ends when every activation it started
//! has reached the target.
//!
//! - `observe_routed/observed/<n>`: `n` observers listed on every answer (0
//!   is the unobserved baseline).
//!
//! The host counts what crosses its socket while the bench runs, and each
//! measurement asserts it: one `invokefunction` in and one
//! `invocationresult` out per call, and no other frame, whatever `n`.
//!
//! `cargo bench -p iii --bench observe_routed_bench`

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

use observe_support::{TestEngine, connect, connect_v2, in_process, registered, spawn_engine};

const OBSERVED: &str = "bench::routed";
const TARGET: &str = "bench::target";

/// Frames the host saw while the bench ran.
#[derive(Default)]
struct Frames {
    invoked: AtomicU64,
    answered: AtomicU64,
    other: AtomicU64,
}

struct Bench {
    t: TestEngine,
    delivered: Arc<AtomicU64>,
    frames: Arc<Frames>,
    // Keeps the observing connection, and with it the observers, alive.
    _observer: observe_support::Client,
}

async fn setup(observers: usize) -> Bench {
    let t = spawn_engine(ObserversConfig {
        // Room for every activation: the bench measures work, not the drop
        // policy.
        default_max_in_flight: 4096,
        max_queue_per_observer: 65_536,
        ..ObserversConfig::default()
    })
    .await;
    let delivered = Arc::new(AtomicU64::new(0));
    let counter = delivered.clone();
    in_process(&t.engine, TARGET, move |_| {
        counter.fetch_add(1, Ordering::Relaxed);
        Value::Null
    });
    let mut host = connect_v2(&t, "host").await;
    host.register_function(OBSERVED).await;
    host.expect("functionregistered", |f| f["type"] == "functionregistered")
        .await;
    registered(&t.engine, "default", OBSERVED).await;

    let mut observer = connect(&t, "observer", true).await;
    let ids: Vec<String> = (0..observers).map(|i| format!("obs-{i}")).collect();
    for id in &ids {
        observer
            .observe_active(json!({
                "id": id,
                "source": { "function_id": OBSERVED },
                "target": { "function_id": TARGET },
            }))
            .await;
    }

    // Every frame from here on is counted. The host acked its observers
    // before they went active, so nothing else is due to it.
    host.discard_received();
    let frames = Arc::new(Frames::default());
    let seen = frames.clone();
    tokio::spawn(async move {
        let listed: Vec<&str> = ids.iter().map(String::as_str).collect();
        while let Some(frame) = host.recv().await {
            if frame["type"] == "invokefunction" && frame["function_id"] == OBSERVED {
                seen.invoked.fetch_add(1, Ordering::Relaxed);
                host.reply_observed(&frame, json!({ "ok": true }), &listed)
                    .await;
                seen.answered.fetch_add(1, Ordering::Relaxed);
            } else {
                seen.other.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
    Bench {
        t,
        delivered,
        frames,
        _observer: observer,
    }
}

/// `iters` calls, one at a time, then until `per_call * iters` activations
/// arrived. Asserts the frames the host saw meanwhile.
async fn run(b: &Bench, iters: u64, per_call: u64) -> Duration {
    let expected = b.delivered.load(Ordering::Relaxed) + per_call * iters;
    let invoked = b.frames.invoked.load(Ordering::Relaxed);
    let answered = b.frames.answered.load(Ordering::Relaxed);
    let deadline = Instant::now() + Duration::from_secs(60);
    let start = Instant::now();
    for i in 0..iters {
        let out = b.t.engine.call(OBSERVED, json!({ "k": i })).await;
        assert!(out.is_ok(), "call failed: {out:?}");
    }
    while b.delivered.load(Ordering::Relaxed) < expected {
        assert!(
            Instant::now() < deadline,
            "activations did not all arrive: {} of {expected}",
            b.delivered.load(Ordering::Relaxed),
        );
        tokio::task::yield_now().await;
    }
    let elapsed = start.elapsed();
    assert_eq!(b.frames.invoked.load(Ordering::Relaxed) - invoked, iters);
    assert_eq!(b.frames.answered.load(Ordering::Relaxed) - answered, iters);
    assert_eq!(
        b.frames.other.load(Ordering::Relaxed),
        0,
        "an observed call took a frame of its own"
    );
    elapsed
}

fn observe_routed(c: &mut Criterion) {
    let rt = Runtime::new().expect("tokio runtime");
    let mut group = c.benchmark_group("observe_routed");
    group.measurement_time(Duration::from_secs(8));
    group.throughput(Throughput::Elements(1));

    for n in [0usize, 1, 8, 64] {
        let bench = rt.block_on(setup(n));
        group.bench_with_input(BenchmarkId::new("observed", n), &n, |b, &n| {
            b.to_async(&rt)
                .iter_custom(|iters| run(&bench, iters, n as u64));
        });
    }
    group.finish();
}

criterion_group!(benches, observe_routed);
criterion_main!(benches);
