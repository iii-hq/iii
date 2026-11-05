//! What an `invocationsobserved` report costs the engine in memory, measured
//! with a counting global allocator (it applies to this test binary only):
//! what reading and decoding its items allocates, and what each
//! report-delivered event holds while it waits in an observer's queue behind
//! a target that never answers.
//!
//! Every scenario runs over real WebSockets, with raw workers that send the
//! frame as the SDKs write it (`type` first) and read activations without
//! keeping them. Each one repeats three times on one engine, after a warm-up
//! round that sized the connections' buffers, and prints the median per item:
//!
//! - `decode`: items the engine reads and drops at once (an unknown `status`,
//!   `INVALID_REPORT`), then one marker item that queues: what reading and
//!   decoding an item allocates.
//! - `queued/worker`, `queued/in-process`: items an observer queues behind a
//!   worker-hosted / in-process target that never answers: what reading them
//!   allocates, and what each queued event holds.
//!
//! `held` is the live-bytes (and live-blocks) delta once the engine settled,
//! corrected for the frame's own buffer, which the test allocated before the
//! window and the client frees inside it. `intake` counts every allocation
//! (a reallocation counts as one) made from sending the frame until the
//! engine settled. `peak` is the highest live-bytes level above the start of
//! the window.
//!
//! Run with `cargo test -p iii --test observe_memory_raw_reports --
//! --test-threads=1 --nocapture`.

mod observe_support;

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::{
        Arc,
        atomic::{AtomicIsize, Ordering},
    },
    time::Duration,
};

use futures_util::{SinkExt, StreamExt, stream::SplitSink};
use iii::engine::{EngineTrait, Handler, RegisterFunctionRequest};
use iii::function::FunctionResult;
use iii::observer::ObserversConfig;
use iii::protocol::ErrorBody;
use observe_support::{OBSERVE_V2, TestEngine, WAIT, eventually, registered, spawn_engine};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use uuid::Uuid;

struct Counting;
static LIVE: AtomicIsize = AtomicIsize::new(0);
static BLOCKS: AtomicIsize = AtomicIsize::new(0);
/// Bytes and blocks ever allocated: what a step allocates, kept or not.
static ALLOCATED: AtomicIsize = AtomicIsize::new(0);
static ALLOCATIONS: AtomicIsize = AtomicIsize::new(0);
/// The highest `LIVE` since [`reset_peak`].
static PEAK: AtomicIsize = AtomicIsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let size = l.size() as isize;
        let live = LIVE.fetch_add(size, Ordering::Relaxed) + size;
        PEAK.fetch_max(live, Ordering::Relaxed);
        BLOCKS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED.fetch_add(size, Ordering::Relaxed);
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

#[derive(Clone, Copy)]
struct Counters {
    live: isize,
    blocks: isize,
    allocated: isize,
    allocations: isize,
}

fn counters() -> Counters {
    Counters {
        live: LIVE.load(Ordering::Relaxed),
        blocks: BLOCKS.load(Ordering::Relaxed),
        allocated: ALLOCATED.load(Ordering::Relaxed),
        allocations: ALLOCATIONS.load(Ordering::Relaxed),
    }
}

fn reset_peak() {
    PEAK.store(LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
}

/// Repetitions of each measurement; the median is reported.
const REPEAT: usize = 3;

/// How long the engine is left alone before a counter is read.
const SETTLE: Duration = Duration::from_millis(300);

/// Frames longer than this are activations: read and dropped unparsed.
const CONTROL_FRAME: usize = 16 * 1024;

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
type Sink = Arc<tokio::sync::Mutex<SplitSink<Ws, WsMessage>>>;

async fn send_text(sink: &Sink, text: String) {
    sink.lock()
        .await
        .send(WsMessage::Text(text.into()))
        .await
        .expect("send a frame");
}

/// A worker speaking the wire protocol directly: it acks the observers added
/// to the functions it hosts, as an SDK whose hooks accept everything does,
/// never answers an activation, and sends prebuilt frames as they are.
struct RawWorker {
    sink: Sink,
    frames: mpsc::UnboundedReceiver<Value>,
    reader: tokio::task::JoinHandle<()>,
}

impl Drop for RawWorker {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

impl RawWorker {
    /// Connects and registers as `name`, declaring `observe.v2`.
    async fn connect(t: &TestEngine, name: &str) -> RawWorker {
        let (ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{}/", t.port))
            .await
            .expect("connect");
        let (sink, mut stream) = ws.split();
        let sink: Sink = Arc::new(tokio::sync::Mutex::new(sink));
        let (tx, frames) = mpsc::unbounded_channel();
        let reader = {
            let sink = sink.clone();
            tokio::spawn(async move {
                while let Some(Ok(message)) = stream.next().await {
                    let WsMessage::Text(text) = message else {
                        continue;
                    };
                    if text.len() > CONTROL_FRAME {
                        continue;
                    }
                    let Ok(frame) = serde_json::from_str::<Value>(&text) else {
                        continue;
                    };
                    if frame["type"] == "observeradded" {
                        let ack = json!({
                            "type": "observerack",
                            "function_id": frame["function_id"],
                            "namespace": frame["namespace"],
                            "id": frame["observer"]["id"],
                            "revision": frame["revision"],
                        });
                        send_text(&sink, ack.to_string()).await;
                    }
                    if tx.send(frame).is_err() {
                        break;
                    }
                }
            })
        };
        let mut worker = RawWorker {
            sink,
            frames,
            reader,
        };
        let registered = worker.expect(|f| f["type"] == "workerregistered").await;
        let id: Uuid = registered["worker_id"].as_str().unwrap().parse().unwrap();
        worker
            .send(json!({
                "type": "invokefunction",
                "function_id": "engine::workers::register",
                "data": { "runtime": "test", "version": "0.25.0", "name": name, "features": OBSERVE_V2 },
                "action": { "type": "void" },
            }))
            .await;
        assert!(
            eventually(|| t.engine.worker_registry.get_worker_name(&id).as_deref() == Some(name))
                .await,
            "`{name}` never finished engine::workers::register"
        );
        worker
    }

    async fn send(&self, frame: Value) {
        send_text(&self.sink, frame.to_string()).await;
    }

    async fn send_text(&self, text: String) {
        send_text(&self.sink, text).await;
    }

    async fn expect(&mut self, matches: impl Fn(&Value) -> bool) -> Value {
        tokio::time::timeout(WAIT, async {
            loop {
                let frame = self.frames.recv().await.expect("socket open");
                if matches(&frame) {
                    return frame;
                }
            }
        })
        .await
        .expect("the expected frame")
    }
}

/// Registers `id` on the engine, in process, as a function whose calls never
/// end.
fn stuck_in_process(t: &TestEngine, id: &str) {
    t.engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: id.to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(|_input: Value| {
            std::future::pending::<FunctionResult<Option<Value>, ErrorBody>>()
        }),
    );
}

/// A JSON object of about `len` bytes shaped like a typical business payload:
/// rows of small fields, so many small nodes.
fn payload(len: usize) -> String {
    let mut rows = Vec::new();
    let mut size = "{\"rows\":[]}".len();
    let mut i = 0u64;
    while size < len {
        let row = json!({
            "id": i,
            "name": format!("row-{i:05}"),
            "tags": ["a", "b"],
            "price": i as f64 * 1.5,
            "active": i.is_multiple_of(2),
        });
        size += row.to_string().len() + 1;
        rows.push(row);
        i += 1;
    }
    json!({ "rows": rows }).to_string()
}

/// One `invocationsobserved` item of `mem::reported`, matched by `obs`, with
/// `input` (JSON text) and a small output.
fn item(status: &str, input: &str) -> String {
    format!(
        r#"{{"function_id":"mem::reported","invocation_id":"{}","matched":["obs"],"status":"{status}","input":{input},"output":{{"ok":true}},"started_at_ms":1,"duration_ms":0.5,"observe_depth":0}}"#,
        Uuid::new_v4()
    )
}

/// The frame an SDK writes for `items`.
fn frame(items: &[String]) -> String {
    format!(
        r#"{{"type":"invocationsobserved","items":[{}]}}"#,
        items.join(",")
    )
}

/// Waits until `obs` runs `running` activations and queues `queued`.
async fn wait_for(t: &TestEngine, running: u32, queued: usize) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    while t.engine.observers.in_flight("obs") != Some((running, queued)) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "obs: {:?}, expected ({running}, {queued})",
            t.engine.observers.in_flight("obs")
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Decode,
    QueuedWorker,
    QueuedInProcess,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Decode => "decode",
            Kind::QueuedWorker => "queued/worker",
            Kind::QueuedInProcess => "queued/in-process",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Sample {
    held: isize,
    held_blocks: isize,
    allocated: isize,
    allocations: isize,
    peak: isize,
}

/// Sends one frame of `items` items with a payload of about `payload_len`
/// bytes, `REPEAT` times after a warm-up round, and prints the medians per
/// item.
async fn measure(kind: Kind, payload_len: usize, items: usize) {
    let t = spawn_engine(ObserversConfig {
        activation_timeout_ms: 3_600_000,
        ..ObserversConfig::default()
    })
    .await;
    let host = RawWorker::connect(&t, "host").await;
    host.send(json!({ "type": "registerfunction", "id": "mem::reported" }))
        .await;
    registered(&t.engine, "default", "mem::reported").await;
    let sink = RawWorker::connect(&t, "sink").await;
    let target = if kind == Kind::QueuedInProcess {
        stuck_in_process(&t, "mem::stuck");
        "mem::stuck"
    } else {
        sink.send(json!({ "type": "registerfunction", "id": "sink::stuck" }))
            .await;
        "sink::stuck"
    };
    registered(&t.engine, "default", target).await;
    sink.send(json!({
        "type": "observe",
        "id": "obs",
        "source": { "function_id": "mem::reported", "capture": ["input", "output"] },
        "target": { "function_id": target },
        "max_in_flight": 1,
    }))
    .await;
    assert!(
        eventually(|| t.engine.observers.get("obs").is_some_and(|o| o.is_active())).await,
        "obs never became active"
    );

    // The observer's only slot, taken for good: everything after it queues.
    host.send_text(frame(&[item("success", r#"{"warm":true}"#)]))
        .await;
    wait_for(&t, 1, 0).await;

    let payload = payload(payload_len);
    let status = if kind == Kind::Decode {
        "paused"
    } else {
        "success"
    };
    let mut queued = 0;
    let mut samples = Vec::new();
    let mut sent_len = 0;
    for round in 0..=REPEAT {
        let mut batch: Vec<String> = (0..items).map(|_| item(status, &payload)).collect();
        if kind == Kind::Decode {
            batch.push(item("success", r#"{"marker":true}"#));
        }
        let mut text = frame(&batch);
        drop(batch);
        text.shrink_to_fit();
        let sent = text.len() as isize;
        sent_len = text.len();
        let expected = queued + if kind == Kind::Decode { 1 } else { items };
        tokio::time::sleep(SETTLE).await;
        let before = counters();
        reset_peak();
        host.send_text(text).await;
        wait_for(&t, 1, expected).await;
        tokio::time::sleep(SETTLE).await;
        let after = counters();
        let peak = PEAK.load(Ordering::Relaxed) - before.live;
        queued = expected;
        let sample = Sample {
            // The frame's buffer: allocated before the window, freed in it.
            held: after.live - before.live + sent,
            held_blocks: after.blocks - before.blocks + 1,
            allocated: after.allocated - before.allocated,
            allocations: after.allocations - before.allocations,
            peak,
        };
        println!(
            "  {} {} KiB round {round}{}: {sample:?}",
            kind.label(),
            payload_len / 1024,
            if round == 0 { " (warm-up)" } else { "" }
        );
        if round > 0 {
            samples.push(sample);
        }
    }
    drop((host, sink));

    let median = |field: fn(&Sample) -> isize| {
        let mut values: Vec<isize> = samples.iter().map(field).collect();
        values.sort_unstable();
        values[values.len() / 2]
    };
    let per_item = |field: fn(&Sample) -> isize| median(field) / items as isize;
    println!(
        "RESULT {:<18} {:>2} KiB payload ({} B JSON), {items} items/frame ({sent_len} B): \
         held/event {} B in {} blocks | intake/item {} B in {} allocations | peak/item {} B",
        kind.label(),
        payload_len / 1024,
        payload.len(),
        per_item(|s| s.held),
        per_item(|s| s.held_blocks),
        per_item(|s| s.allocated),
        per_item(|s| s.allocations),
        per_item(|s| s.peak),
    );
}

#[tokio::test]
async fn decode_4k() {
    measure(Kind::Decode, 4 * 1024, 200).await;
}

#[tokio::test]
async fn decode_64k() {
    measure(Kind::Decode, 64 * 1024, 32).await;
}

#[tokio::test]
async fn queued_worker_4k() {
    measure(Kind::QueuedWorker, 4 * 1024, 200).await;
}

#[tokio::test]
async fn queued_worker_64k() {
    measure(Kind::QueuedWorker, 64 * 1024, 32).await;
}

#[tokio::test]
async fn queued_in_process_4k() {
    measure(Kind::QueuedInProcess, 4 * 1024, 200).await;
}

#[tokio::test]
async fn queued_in_process_64k() {
    measure(Kind::QueuedInProcess, 64 * 1024, 32).await;
}
