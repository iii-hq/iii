//! What an observer activation costs when a worker connection hosts its
//! target: the engine builds the target's argument from the event and sends
//! it over the worker's WebSocket, once per observer.
//!
//! Each iteration emits [`BURST`] events of an engine-hosted source, each
//! carrying a payload of about `size` bytes, to `n` engine-owned observers
//! whose targets one worker connection hosts. It ends when the worker has
//! received every activation (`BURST * n` `invokefunction` frames). The
//! worker declares the features a 0.26 SDK does, and answers each frame,
//! reading only its `invocation_id` and `function_id`.
//!
//! - `observe_activation_ws/envelope/<size>/<n>`: the default `Observed`
//!   envelope (input, output and caller captured; an event source's input
//!   and output are the same event).
//! - `observe_activation_ws/input/<size>/<n>`: `deliver: input`, the event
//!   alone.
//!
//! `cargo bench -p iii --bench observe_activation_ws_bench`

#[path = "../tests/observe_support/mod.rs"]
mod observe_support;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use futures_util::{SinkExt, StreamExt};
use iii::observer::Deliver;
use iii::observer::{EngineObserver, ObserversConfig, events::register_event_source};
use iii::protocol::DEFAULT_NAMESPACE;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::runtime::Runtime;
use tokio::sync::{Notify, mpsc};
use tokio_tungstenite::tungstenite::Message as WsMessage;

use observe_support::{OBSERVE_V2, TestEngine, eventually, registered, spawn_engine};

const SOURCE: &str = "bench::on-event";

/// Events emitted back to back per iteration.
const BURST: u64 = 8;

const SIZES: [(&str, usize); 3] = [("256B", 256), ("4KiB", 4 * 1024), ("64KiB", 64 * 1024)];

const FANOUTS: [usize; 3] = [1, 16, 64];

/// An order-like JSON document of about `size` bytes once encoded.
fn payload(size: usize) -> Value {
    let item = |i: usize| {
        json!({
            "id": i,
            "sku": format!("sku-{i:06}"),
            "name": format!("Item number {i}"),
            "price": 10.25 + i as f64,
            "quantity": i % 7,
            "active": i.is_multiple_of(2),
            "tags": ["alpha", "beta", "gamma"],
        })
    };
    let mut doc = json!({
        "order_id": "ord-0001",
        "customer": { "id": 42, "email": "someone@example.com" },
        "items": [],
    });
    let mut i = 0;
    while serde_json::to_string(&doc).unwrap().len() < size {
        doc["items"].as_array_mut().unwrap().push(item(i));
        i += 1;
    }
    doc
}

/// The fields of a frame the worker reads; the rest (the activation's
/// `data`) is skipped.
#[derive(Deserialize)]
struct Head {
    #[serde(rename = "type")]
    kind: String,
    invocation_id: Option<String>,
    function_id: Option<String>,
}

/// A worker connection hosting the observers' targets.
struct Targets {
    received: Arc<AtomicU64>,
    notify: Arc<Notify>,
    reader: tokio::task::JoinHandle<()>,
    writer: tokio::task::JoinHandle<()>,
}

impl Drop for Targets {
    fn drop(&mut self) {
        self.reader.abort();
        self.writer.abort();
    }
}

async fn host_targets(port: u16, ids: &[String]) -> Targets {
    let (ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/"))
        .await
        .expect("connect");
    let (mut sink, mut stream) = ws.split();
    loop {
        match stream.next().await {
            Some(Ok(WsMessage::Text(text))) if text.contains("workerregistered") => break,
            Some(Ok(_)) => {}
            other => panic!("no workerregistered: {other:?}"),
        }
    }
    let mut frames = vec![json!({
        "type": "invokefunction",
        "function_id": "engine::workers::register",
        "data": { "runtime": "bench", "version": "0.26.0", "name": "targets", "features": OBSERVE_V2 },
        "action": { "type": "void" },
    })];
    frames.extend(
        ids.iter()
            .map(|id| json!({ "type": "registerfunction", "id": id })),
    );
    for frame in frames {
        sink.send(WsMessage::Text(frame.to_string().into()))
            .await
            .expect("send");
    }

    // Answers go out in batches: one flush for every answer queued meanwhile.
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let writer = tokio::spawn(async move {
        while let Some(answer) = rx.recv().await {
            if sink.feed(WsMessage::Text(answer.into())).await.is_err() {
                return;
            }
            while let Ok(answer) = rx.try_recv() {
                if sink.feed(WsMessage::Text(answer.into())).await.is_err() {
                    return;
                }
            }
            if sink.flush().await.is_err() {
                return;
            }
        }
    });
    let received = Arc::new(AtomicU64::new(0));
    let notify = Arc::new(Notify::new());
    let reader = {
        let received = received.clone();
        let notify = notify.clone();
        tokio::spawn(async move {
            while let Some(Ok(message)) = stream.next().await {
                let WsMessage::Text(text) = message else {
                    continue;
                };
                let Ok(head) = serde_json::from_str::<Head>(&text) else {
                    continue;
                };
                if head.kind != "invokefunction" {
                    continue;
                }
                if let (Some(invocation_id), Some(function_id)) =
                    (head.invocation_id, head.function_id)
                {
                    let answer = json!({
                        "type": "invocationresult",
                        "invocation_id": invocation_id,
                        "function_id": function_id,
                        "result": null,
                    });
                    let _ = tx.send(answer.to_string());
                }
                received.fetch_add(1, Ordering::Release);
                notify.notify_one();
            }
        })
    };
    Targets {
        received,
        notify,
        reader,
        writer,
    }
}

struct Bench {
    t: TestEngine,
    targets: Targets,
}

async fn setup(fanout: usize, deliver: Deliver) -> Bench {
    let t = spawn_engine(ObserversConfig {
        // Room for every activation of a burst: the bench measures work, not
        // the drop policy.
        default_max_in_flight: 4096,
        max_queue_per_observer: 65_536,
        max_per_connection: 8192,
        max_per_function: 8192,
        ..ObserversConfig::default()
    })
    .await;
    register_event_source(&t.engine, SOURCE, "bench source", json!({}));
    let ids: Vec<String> = (0..fanout).map(|i| format!("bench::target-{i}")).collect();
    let targets = host_targets(t.port, &ids).await;
    for id in &ids {
        registered(&t.engine, DEFAULT_NAMESPACE, id).await;
    }
    for (i, id) in ids.iter().enumerate() {
        t.engine
            .observers
            .observe_as_engine(EngineObserver {
                id: format!("bench-observer-{i}"),
                function: (DEFAULT_NAMESPACE.to_string(), SOURCE.to_string()),
                args: json!({}),
                filters: None,
                target: (DEFAULT_NAMESPACE.to_string(), id.clone()),
                deliver,
                metadata: None,
            })
            .expect("observe the source");
    }
    let engine = t.engine.clone();
    assert!(
        eventually(|| (0..fanout).all(|i| {
            engine
                .observers
                .get(&format!("bench-observer-{i}"))
                .is_some_and(|observer| observer.is_active())
        }))
        .await,
        "the observers never became active"
    );
    Bench { t, targets }
}

/// `iters` bursts, each timed from its first emit until the worker received
/// all of its activations. The events are copied before the clock starts.
async fn run(b: &Bench, iters: u64, fanout: u64, payload: &Value) -> Duration {
    let mut total = Duration::ZERO;
    for _ in 0..iters {
        let events: Vec<Value> = (0..BURST).map(|_| payload.clone()).collect();
        let expected = b.targets.received.load(Ordering::Acquire) + BURST * fanout;
        let deadline = Instant::now() + Duration::from_secs(30);
        let start = Instant::now();
        for event in events {
            b.t.engine
                .observers
                .emit(DEFAULT_NAMESPACE, SOURCE, move || Some(event));
        }
        while b.targets.received.load(Ordering::Acquire) < expected {
            let _ =
                tokio::time::timeout(Duration::from_millis(50), b.targets.notify.notified()).await;
            assert!(
                Instant::now() < deadline,
                "activations did not all arrive: {} of {expected}",
                b.targets.received.load(Ordering::Acquire),
            );
        }
        total += start.elapsed();
    }
    total
}

fn observe_activation_ws(c: &mut Criterion) {
    let rt = Runtime::new().expect("tokio runtime");
    let mut group = c.benchmark_group("observe_activation_ws");
    group.sample_size(30);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(5));
    let payloads: Vec<(&str, Value)> = SIZES
        .iter()
        .map(|(label, size)| (*label, payload(*size)))
        .collect();
    for (label, deliver) in [("envelope", Deliver::Payload), ("input", Deliver::Input)] {
        for fanout in FANOUTS {
            let bench = rt.block_on(setup(fanout, deliver));
            for (size, payload) in &payloads {
                group.throughput(Throughput::Elements(BURST * fanout as u64));
                group.bench_with_input(
                    BenchmarkId::new(format!("{label}/{size}"), fanout),
                    &fanout,
                    |b, &fanout| {
                        b.to_async(&rt)
                            .iter_custom(|iters| run(&bench, iters, fanout as u64, payload));
                    },
                );
            }
        }
    }
    group.finish();
}

criterion_group!(benches, observe_activation_ws);
criterion_main!(benches);
