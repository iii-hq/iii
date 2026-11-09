//! Bytes and allocations an observed call's event holds while it waits in an
//! observer's queue (text-events experiment).
//!
//! An in-process source is observed by one observer that captures the input
//! (the default capture), so the call keeps the whole input, as JSON text
//! when it is large. The observer's target never answers and the observer
//! runs one activation at a time, so every event after the first waits in its
//! queue. The target is hosted by a worker connection (the activation is an
//! encoded frame) or registered in-process (the activation takes a `Value`),
//! and the observer filters on an input field or not.
//!
//! For each case the engine is settled (no call in flight, every event
//! evaluated and queued) before and after `EVENTS` more calls; the deltas of
//! the live bytes, the live blocks and the allocations made, divided by
//! `EVENTS`, are what one queued event costs. Each case is the median of 3
//! runs, each on a new engine.
//!
//! `cargo test -p iii --test observe_memory_text_events -- --test-threads=1 --nocapture`

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::{
        Arc,
        atomic::{AtomicIsize, Ordering},
    },
    time::{Duration, Instant},
};

use iii::engine::{Engine, EngineTrait, Handler, Outbound, RegisterFunctionRequest};
use iii::function::FunctionResult;
use iii::observer::{ObserveRequest, ObserversConfig};
use iii::protocol::{Message, ObserveSource, ObserveTarget, ObserverState};
use iii::worker_connections::WorkerConnection;
use serde_json::{Value, json};
use tokio::sync::{Semaphore, mpsc};
use uuid::Uuid;

struct Counting;

/// Bytes allocated and not freed.
static LIVE: AtomicIsize = AtomicIsize::new(0);
/// Blocks allocated and not freed.
static BLOCKS: AtomicIsize = AtomicIsize::new(0);
/// Blocks allocated, freed or not.
static ALLOCATIONS: AtomicIsize = AtomicIsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE.fetch_add(l.size() as isize, Ordering::Relaxed);
        BLOCKS.fetch_add(1, Ordering::Relaxed);
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size() as isize, Ordering::Relaxed);
        BLOCKS.fetch_sub(1, Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

const SOURCE: &str = "mem::src";
const WORKER_TARGET: &str = "mem::dst";
const IN_PROCESS_TARGET: &str = "mem::sink";
const OBSERVER: &str = "obs";

/// Calls made before the first reading: the first activation runs (and never
/// ends), the rest wait in the queue.
const WARM_UP: usize = 2;

/// Calls measured: each one's event waits in the observer's queue.
const EVENTS: usize = 64;

const RUNS: usize = 3;

#[derive(Clone, Copy, Debug)]
enum Target {
    /// Hosted by a worker connection: its activations are encoded frames.
    Worker,
    /// Registered in-process: its activations take a `Value`.
    InProcess,
}

#[derive(Clone, Copy)]
struct Snapshot {
    live: isize,
    blocks: isize,
    allocations: isize,
}

fn snapshot() -> Snapshot {
    Snapshot {
        live: LIVE.load(Ordering::SeqCst),
        blocks: BLOCKS.load(Ordering::SeqCst),
        allocations: ALLOCATIONS.load(Ordering::SeqCst),
    }
}

/// Per queued event: live bytes, live blocks, allocations made.
#[derive(Clone, Copy)]
struct PerEvent {
    bytes: f64,
    blocks: f64,
    allocations: f64,
}

/// A JSON object of about `size` bytes encoded: an order with many small
/// items, the shape of a typical large payload.
fn payload(size: usize) -> Value {
    let item = |i: usize| {
        json!({
            "id": i,
            "sku": format!("SKU-{i:06}"),
            "qty": i % 7,
            "price": 12.5,
            "tags": ["red", "large"],
        })
    };
    let mut items = Vec::new();
    let mut length = r#"{"kind":"order","items":[]}"#.len();
    while length < size {
        let next = item(items.len());
        length += serde_json::to_string(&next).unwrap().len() + 1;
        items.push(next);
    }
    json!({ "kind": "order", "items": items })
}

async fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

/// Lets the runtime finish what it is doing (the evaluator's last steps,
/// dropped temporaries) before a reading.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(200)).await;
}

/// One run: what each of `EVENTS` queued events costs.
async fn run(target: Target, size: usize, filters: Option<Value>) -> PerEvent {
    let engine = Arc::new(Engine::new());
    engine.observers.set_config(ObserversConfig {
        activation_timeout_ms: 600_000,
        ..ObserversConfig::default()
    });
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: SOURCE.to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(|_input: Value| async {
            FunctionResult::Success(Some(json!({ "ok": true })))
        }),
    );

    // The target never answers while `gate` is open.
    let gate = Arc::new(Semaphore::new(0));
    let (host_tx, host_rx) = mpsc::channel::<Outbound>(64);
    let host = WorkerConnection::new(host_tx);
    let target_id = match target {
        Target::Worker => {
            engine.worker_registry.register_worker(host.clone());
            let register: Message = serde_json::from_value(json!({
                "type": "registerfunction",
                "id": WORKER_TARGET,
            }))
            .unwrap();
            engine.router_msg(&host, &register).await.unwrap();
            WORKER_TARGET
        }
        Target::InProcess => {
            let gate = gate.clone();
            engine.register_function_handler(
                RegisterFunctionRequest {
                    function_id: IN_PROCESS_TARGET.to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                },
                Handler::new(move |_input: Value| {
                    let gate = gate.clone();
                    async move {
                        let _ = gate.acquire().await;
                        FunctionResult::Success(None)
                    }
                }),
            );
            IN_PROCESS_TARGET
        }
    };

    let (observing_tx, observing_rx) = mpsc::channel::<Outbound>(1024);
    let observing = WorkerConnection::new(observing_tx);
    engine.worker_registry.register_worker(observing.clone());
    engine
        .observers
        .observe(
            &engine,
            &observing,
            ObserveRequest {
                id: OBSERVER.to_string(),
                source: ObserveSource {
                    function_id: SOURCE.to_string(),
                    namespace: None,
                    args: None,
                    filters,
                    capture: None,
                    on: None,
                    deliver: None,
                    raw_input: false,
                },
                target: ObserveTarget {
                    function_id: target_id.to_string(),
                    namespace: None,
                },
                metadata: None,
                max_fires: None,
                expires_in_ms: None,
                max_in_flight: Some(1),
            },
        )
        .await;
    until("the observer to be active", || {
        engine.observers.get(OBSERVER).map(|o| o.state()) == Some(ObserverState::Active)
    })
    .await;

    let body = payload(size);
    for _ in 0..WARM_UP {
        engine
            .call(SOURCE, &body)
            .await
            .expect("the source answers");
    }
    until("the warm-up events to queue", || {
        engine.observers.in_flight(OBSERVER) == Some((1, WARM_UP - 1))
    })
    .await;
    settle().await;

    let before = snapshot();
    for _ in 0..EVENTS {
        engine
            .call(SOURCE, &body)
            .await
            .expect("the source answers");
    }
    until("the measured events to queue", || {
        engine.observers.in_flight(OBSERVER) == Some((1, WARM_UP - 1 + EVENTS))
    })
    .await;
    settle().await;
    let after = snapshot();

    // Let go of everything the run holds: the observer's disposal drains its
    // queue, and a closed gate lets an in-process activation end.
    engine
        .observers
        .connection_closed(observing.id, Uuid::new_v4());
    gate.close();
    settle().await;
    drop((host_rx, observing_rx, host, observing));

    let events = EVENTS as f64;
    PerEvent {
        bytes: (after.live - before.live) as f64 / events,
        blocks: (after.blocks - before.blocks) as f64 / events,
        allocations: (after.allocations - before.allocations) as f64 / events,
    }
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    values[values.len() / 2]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bytes_held_per_queued_event() {
    iii::workers::observability::metrics::ensure_default_meter();
    let filter = json!({ "input.kind": { "$eq": "order" } });
    println!();
    println!(
        "per queued event, median of {RUNS} runs of {EVENTS} events \
         (live bytes, live blocks, allocations made)"
    );
    println!(
        "{:<10} {:<10} {:>8} {:>12} {:>12} {:>12}",
        "target", "filter", "json", "bytes", "blocks", "allocations"
    );
    for target in [Target::Worker, Target::InProcess] {
        for filtered in [false, true] {
            for size in [4 * 1024, 64 * 1024] {
                let mut runs = Vec::new();
                for _ in 0..RUNS {
                    runs.push(run(target, size, filtered.then(|| filter.clone())).await);
                }
                let json = serde_json::to_string(&payload(size)).unwrap().len();
                println!(
                    "{:<10} {:<10} {:>8} {:>12.0} {:>12.1} {:>12.1}",
                    format!("{target:?}"),
                    if filtered { "input.kind" } else { "none" },
                    json,
                    median(runs.iter().map(|r| r.bytes).collect()),
                    median(runs.iter().map(|r| r.blocks).collect()),
                    median(runs.iter().map(|r| r.allocations).collect()),
                );
            }
        }
    }
}
