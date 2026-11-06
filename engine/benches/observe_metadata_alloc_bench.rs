//! What keeping a call's metadata for its observers costs, in heap bytes
//! and allocations per observed call (decision 62): a measure the machine's
//! load does not move, unlike the latency of `observe_routed_bench`.
//!
//! An in-process function is observed by `n` observers capturing
//! `[input, output, caller, metadata]`; their target runs in-process. Every
//! call carries no metadata, ~60 B (a routing key) or ~4 KB (an opaque
//! context). The caller's own metadata values are built before the measured
//! window, so what is counted is the engine's work: the call, the copies the
//! observed call keeps, the evaluation and every activation's envelope.
//! Wall time per call is printed too, for reference only.
//!
//! `cargo bench -p iii --bench observe_metadata_alloc_bench`

#[path = "../tests/observe_support/mod.rs"]
mod observe_support;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use iii::engine::EngineTrait;
use iii::observer::ObserversConfig;
use serde_json::{Value, json};

use observe_support::{connect, in_process, spawn_engine};

struct Counting;

static BYTES: AtomicU64 = AtomicU64::new(0);
static ALLOCS: AtomicU64 = AtomicU64::new(0);

// SAFETY: every call is forwarded to `System` unchanged; the counters only
// observe the requested sizes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

const OBSERVED: &str = "bench::observed";
const TARGET: &str = "bench::target";
/// Calls per measurement, waited on for the evaluator every [`BATCH`].
const CALLS: u64 = 4096;
const BATCH: u64 = 512;

fn metadata(label: &str) -> Option<Value> {
    let key = "0d1f5c2e-6a4b-4f7e-9c3d-2b8a7e6f5d4c";
    match label {
        "60b" => Some(json!({ "for_observer": key })),
        "4k" => Some(json!({
            "for_observer": key,
            "context": (0..64)
                .map(|i| (format!("key-{i:02}"), json!("v".repeat(48))))
                .collect::<serde_json::Map<_, _>>(),
        })),
        _ => None,
    }
}

/// `(bytes, allocations, wall time)` per call, over [`CALLS`] calls.
async fn measure(observers: usize, label: &str) -> (f64, f64, Duration) {
    let t = spawn_engine(ObserversConfig {
        default_max_in_flight: 4096,
        max_queue_per_observer: 65_536,
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
        observer
            .observe_active(json!({
                "id": format!("obs-{i}"),
                "source": {
                    "function_id": OBSERVED,
                    "capture": ["input", "output", "caller", "metadata"],
                },
                "target": { "function_id": TARGET },
            }))
            .await;
    }

    let run = |calls: u64, metas: Vec<Option<Value>>| {
        let t = &t;
        let delivered = &delivered;
        async move {
            let evaluated = t.engine.observers.evaluated();
            let expected = delivered.load(Ordering::Relaxed) + observers as u64 * calls;
            let deadline = Instant::now() + Duration::from_secs(120);
            for (i, meta) in (0..calls).zip(metas) {
                let out = t
                    .engine
                    .call_with_metadata(OBSERVED, json!({ "k": i }), meta)
                    .await;
                assert!(out.is_ok(), "call failed");
                if (i + 1) % BATCH == 0 {
                    while t.engine.observers.evaluated() < evaluated + i + 1 {
                        assert!(Instant::now() < deadline, "the evaluator never caught up");
                        tokio::task::yield_now().await;
                    }
                }
            }
            while delivered.load(Ordering::Relaxed) < expected {
                assert!(Instant::now() < deadline, "activations did not all arrive");
                tokio::task::yield_now().await;
            }
        }
    };

    // Warm-up: connections, maps and pools reach their steady size.
    let metas = (0..CALLS).map(|_| metadata(label)).collect();
    run(CALLS, metas).await;

    let metas = (0..CALLS).map(|_| metadata(label)).collect();
    let (bytes, allocs) = (BYTES.load(Ordering::SeqCst), ALLOCS.load(Ordering::SeqCst));
    let start = Instant::now();
    run(CALLS, metas).await;
    let elapsed = start.elapsed();
    let bytes = BYTES.load(Ordering::SeqCst) - bytes;
    let allocs = ALLOCS.load(Ordering::SeqCst) - allocs;
    drop(observer);
    (
        bytes as f64 / CALLS as f64,
        allocs as f64 / CALLS as f64,
        elapsed / CALLS as u32,
    )
}

fn main() {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    println!("observers  metadata  bytes/call  allocs/call  wall/call");
    for observers in [1usize, 8] {
        for label in ["none", "60b", "4k"] {
            let (bytes, allocs, wall) = rt.block_on(measure(observers, label));
            println!("{observers:>9}  {label:>8}  {bytes:>10.0}  {allocs:>11.1}  {wall:>9.2?}");
        }
    }
}
