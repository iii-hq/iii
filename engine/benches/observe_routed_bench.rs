//! What observing a function costs a routed call whose host declared
//! `observe.v2` (v2 §8.2, decision 55): the engine evaluates the observers'
//! filters with the input it kept and the answer's result, and the host's
//! answer says among which observers (`invocationresult.matched`), so an
//! observed call takes the same frames as an unobserved one.
//!
//! A WebSocket worker that declared `observe.v2` hosts the function and
//! answers every call at once. The observers' target runs in-process. An
//! iteration is one call, from the engine to the host and back, and ends
//! when every activation it started has reached the target.
//!
//! - `observe_routed/observed/<n>`: `n` observers without filters, the
//!   answer the result alone (a function without a match hook): the engine
//!   evaluates every observer (0 is the unobserved baseline).
//! - `observe_routed/matched/<n>`: the same observers, every one named in
//!   the answer's `matched` (a match hook that matched them all).
//! - `observe_routed/filtered/<n>`: `n` observers each filtering on its own
//!   value of `input.k`, the answer the result alone: every call matches one
//!   observer, found through the function's filter index (the load test's
//!   `filters` scenario).
//!
//! The host counts what crosses its socket while the bench runs, and each
//! measurement asserts it: one `invokefunction` in and one
//! `invocationresult` out per call, and no other frame, whatever `n`. After
//! each case the bench prints the totals and the frames per call.
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
    /// Calls the bench made.
    calls: AtomicU64,
    // Keeps the observing connection, and with it the observers, alive.
    _observer: observe_support::Client,
}

/// The observers a case creates, and what the host answers.
#[derive(Clone, Copy, PartialEq)]
enum Case {
    /// No filters; the answer carries the result alone.
    Observed,
    /// No filters; the answer names every observer in `matched`.
    Matched,
    /// Observer `i` filters on `input.k == i`; the answer carries the result
    /// alone.
    Filtered,
}

async fn setup(observers: usize, case: Case) -> Bench {
    let t = spawn_engine(ObserversConfig {
        // Room for every activation: the bench measures work, not the drop
        // policy.
        default_max_in_flight: 4096,
        max_queue_per_observer: 65_536,
        // Every observer comes from one connection.
        max_per_connection: 8192,
        max_per_function: 8192,
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
    for (i, id) in ids.iter().enumerate() {
        let source = if case == Case::Filtered {
            json!({ "function_id": OBSERVED, "filters": { "input.k": { "$eq": i } } })
        } else {
            json!({ "function_id": OBSERVED })
        };
        observer
            .observe_active(json!({
                "id": id,
                "source": source,
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
                // Counted before it is sent: the call it answers returns as
                // soon as the engine reads it, and `run` then reads the count.
                seen.answered.fetch_add(1, Ordering::Relaxed);
                if case == Case::Matched {
                    host.reply_matched(&frame, json!({ "ok": true }), &listed)
                        .await;
                } else {
                    host.reply(&frame, json!({ "ok": true })).await;
                }
            } else {
                seen.other.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
    Bench {
        t,
        delivered,
        frames,
        calls: AtomicU64::new(0),
        _observer: observer,
    }
}

/// `iters` calls, one at a time (call `i` with `input.k = i % keys`), then
/// until `per_call * iters` activations arrived. Asserts the frames the host
/// saw meanwhile.
async fn run(b: &Bench, iters: u64, per_call: u64, keys: u64) -> Duration {
    let expected = b.delivered.load(Ordering::Relaxed) + per_call * iters;
    let invoked = b.frames.invoked.load(Ordering::Relaxed);
    let answered = b.frames.answered.load(Ordering::Relaxed);
    let deadline = Instant::now() + Duration::from_secs(60);
    let start = Instant::now();
    for i in 0..iters {
        let k = if keys > 0 { i % keys } else { i };
        let out = b.t.engine.call(OBSERVED, json!({ "k": k })).await;
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
    b.calls.fetch_add(iters, Ordering::Relaxed);
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

    let cases = [
        (Case::Observed, "observed", &[0usize, 1, 8, 64][..]),
        (Case::Matched, "matched", &[1, 8, 64][..]),
        (Case::Filtered, "filtered", &[16, 256, 1024, 4000][..]),
    ];
    for (case, name, sizes) in cases {
        for &n in sizes {
            let bench = rt.block_on(setup(n, case));
            let (per_call, keys) = match case {
                Case::Filtered => (1, n as u64),
                Case::Observed | Case::Matched => (n as u64, 0),
            };
            group.bench_with_input(BenchmarkId::new(name, n), &n, |b, _| {
                b.to_async(&rt)
                    .iter_custom(|iters| run(&bench, iters, per_call, keys));
            });
            let calls = bench.calls.load(Ordering::Relaxed);
            let invoked = bench.frames.invoked.load(Ordering::Relaxed);
            let answered = bench.frames.answered.load(Ordering::Relaxed);
            let frames_per_call = |frames: u64| frames as f64 / calls.max(1) as f64;
            eprintln!(
                "observe_routed/{name}/{n}: {calls} calls; at the host {invoked} invokefunction in ({:.3} per call), {answered} invocationresult out ({:.3} per call), {} other frames",
                frames_per_call(invoked),
                frames_per_call(answered),
                bench.frames.other.load(Ordering::Relaxed),
            );
        }
    }
    group.finish();
}

criterion_group!(benches, observe_routed);
criterion_main!(benches);
