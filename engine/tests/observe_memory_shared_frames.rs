//! Memory a worker connection holds for the observer activations queued to
//! it, measured with a counting global allocator (this test binary only).
//!
//! One event of `size` bytes is observed by `k` observers whose target a
//! worker connection hosts. The connection's writer is stuck (a worker that
//! stops reading), so the `k` activations stay in its channel, which holds
//! 64 messages as a real connection's does. The figures are what the engine
//! holds once it settles, minus what it held before the caller built the
//! event: the queued frames, plus the invocations waiting for their results.
//!
//! `cargo test -p iii --test observe_memory_shared_frames -- --test-threads=1 --nocapture`

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
use serde_json::json;
use tokio::sync::mpsc;

struct Counting;
static LIVE: AtomicIsize = AtomicIsize::new(0);
static BLOCKS: AtomicIsize = AtomicIsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE.fetch_add(l.size() as isize, Ordering::Relaxed);
        BLOCKS.fetch_add(1, Ordering::Relaxed);
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
const SINK: &str = "mem::sink";

/// Messages a worker connection's channel holds.
const CHANNEL: usize = 64;

fn counters() -> (isize, isize) {
    (LIVE.load(Ordering::SeqCst), BLOCKS.load(Ordering::SeqCst))
}

async fn settle() {
    tokio::time::sleep(Duration::from_millis(200)).await;
}

fn request(i: usize) -> ObserveRequest {
    ObserveRequest {
        id: format!("obs-{i}"),
        source: ObserveSource {
            function_id: SOURCE.to_string(),
            namespace: None,
            args: None,
            filters: None,
            capture: None,
            on: None,
            deliver: None,
            raw_input: false,
        },
        target: ObserveTarget {
            function_id: SINK.to_string(),
            namespace: None,
        },
        metadata: None,
        max_fires: None,
        expires_in_ms: None,
        max_in_flight: None,
    }
}

/// An event whose input is `size` bytes of JSON.
fn event(size: usize) -> serde_json::Value {
    // `{"blob":"` and `"}` are 11 bytes.
    json!({ "blob": "x".repeat(size - 11) })
}

/// Bytes and allocations the engine holds once `k` activations of one
/// `size`-byte event are queued to a connection that does not read.
async fn queued(size: usize, k: usize) -> (isize, isize) {
    let engine = Arc::new(Engine::new());
    engine.observers.set_config(ObserversConfig::default());
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: SOURCE.to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(|_input| async { FunctionResult::Success(None) }),
    );

    // The target's connection: nothing reads its channel.
    let (target_tx, target_rx) = mpsc::channel::<Outbound>(CHANNEL);
    let target = WorkerConnection::new(target_tx);
    engine.worker_registry.register_worker(target.clone());
    engine
        .router_msg(
            &target,
            &Message::RegisterFunction {
                id: SINK.to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
                invocation: None,
                args_format: None,
                private: false,
                observable: true,
                observe: None,
            },
        )
        .await
        .expect("register the target");

    // The observers' owner: its channel is drained.
    let (watcher_tx, mut watcher_rx) = mpsc::channel::<Outbound>(1 << 12);
    tokio::spawn(async move { while watcher_rx.recv().await.is_some() {} });
    let watcher = WorkerConnection::new(watcher_tx);
    engine.worker_registry.register_worker(watcher.clone());
    for i in 0..k {
        engine
            .observers
            .observe(&engine, &watcher, request(i))
            .await;
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while (0..k).any(|i| {
        engine.observers.get(&format!("obs-{i}")).map(|o| o.state()) != Some(ObserverState::Active)
    }) {
        assert!(Instant::now() < deadline, "observers never became active");
        tokio::time::sleep(Duration::from_millis(2)).await;
    }

    settle().await;
    let (live, blocks) = counters();

    // Built after the baseline: the engine frees it once it has dispatched
    // the event, so the caller's copy is not subtracted from what it holds.
    let input = event(size);
    assert_eq!(input.to_string().len(), size);
    engine
        .call(SOURCE, input)
        .await
        .expect("the source answers");
    let deadline = Instant::now() + Duration::from_secs(10);
    while target_rx.len() < k {
        assert!(
            Instant::now() < deadline,
            "only {} activations queued",
            target_rx.len()
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    settle().await;
    let (live_after, blocks_after) = counters();
    assert_eq!(target_rx.len(), k, "every activation is queued, no more");

    drop(target_rx);
    drop(target);
    drop(watcher);
    drop(engine);
    (live_after - live, blocks_after - blocks)
}

fn median(mut samples: Vec<isize>) -> isize {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bytes_held_for_queued_activations() {
    iii::workers::observability::metrics::ensure_default_meter();
    // Warms what the first engine allocates once per process.
    queued(64, 1).await;

    println!(
        "{:>8} {:>4} {:>12} {:>8} {:>14} {:>12}",
        "event", "k", "live bytes", "allocs", "bytes/activ.", "allocs/activ."
    );
    for size in [64, 4 * 1024, 64 * 1024] {
        for k in [16, CHANNEL] {
            let mut live = Vec::new();
            let mut blocks = Vec::new();
            for _ in 0..3 {
                let (l, b) = queued(size, k).await;
                live.push(l);
                blocks.push(b);
            }
            let (live, blocks) = (median(live), median(blocks));
            println!(
                "{:>8} {:>4} {:>12} {:>8} {:>14} {:>12.1}",
                size,
                k,
                live,
                blocks,
                live / k as isize,
                blocks as f64 / k as f64
            );
        }
    }
}
