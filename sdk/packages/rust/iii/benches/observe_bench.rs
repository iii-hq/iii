//! observe hot paths on a Rust host (README §8 performance budget).
//!
//! - `is_observed()` with nobody observing: one atomic load, no allocation;
//! - a local call through `trigger()` with no observers: < 2 µs SDK overhead
//!   on top of the handler (compare with `handler_alone`);
//! - a local call with 10 observers, none matching: < 5 µs;
//! - a local call with 1 matching observer: 1 report item, batched.
//!
//! A local call needs the engine's `functionregistered`, so a small in-process
//! fake engine confirms the functions with the observers each case needs.
//!
//! `cargo bench -p iii-sdk --bench observe_bench`

use std::{
    hint::black_box,
    net::SocketAddr,
    time::{Duration, Instant},
};

use criterion::{Criterion, criterion_group, criterion_main};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use iii_helpers::observability::OtelConfig;
use iii_sdk::protocol::TriggerRequest;
use iii_sdk::runtime::{FunctionRef, WorkerMetadata};
use iii_sdk::{
    Error, IIIClient, InitOptions, RegisterFunction, WorkerIdentityMode, register_worker,
};

/// Observers the fake engine sends with each function's `functionregistered`.
fn observers_for(function_id: &str) -> Value {
    match function_id {
        "bench::ten-none" => Value::Array(
            (0..10)
                .map(|i| {
                    json!({
                        "id": format!("obs-{i}"), "args": {}, "target": "bench::target",
                        "filters": { "$and": [{ "input.n": { "$eq": -1 - i } }] },
                        "capture": ["input"], "on": "success"
                    })
                })
                .collect(),
        ),
        "bench::one-match" => json!([{
            "id": "obs-all", "args": {}, "target": "bench::target",
            "filters": { "$and": [{ "input.n": { "$gte": 0 } }] },
            "capture": ["input", "output"], "on": "success"
        }]),
        _ => json!([]),
    }
}

/// Accepts the worker, advertises observe, and confirms every function it
/// registers. Frames from the worker (reports included) are read and dropped.
fn spawn_fake_engine() -> SocketAddr {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(listener.local_addr().unwrap()).unwrap();
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    let Ok(ws) = tokio_tungstenite::accept_async(socket).await else {
                        return;
                    };
                    let (mut out, mut inbound) = ws.split();
                    let hello = json!({
                        "type": "workerregistered", "worker_id": "bench-worker",
                        "features": ["observe.v1", "local-dispatch.v1"]
                    });
                    let _ = out.send(WsMessage::Text(hello.to_string().into())).await;
                    while let Some(Ok(frame)) = inbound.next().await {
                        let WsMessage::Text(text) = frame else {
                            continue;
                        };
                        let Ok(message) = serde_json::from_str::<Value>(text.as_str()) else {
                            continue;
                        };
                        if message["type"] == "registerfunction" {
                            let id = message["id"].as_str().unwrap_or_default();
                            let confirm = json!({
                                "type": "functionregistered", "id": id, "namespace": "default",
                                "revision": 1, "local_dispatch": true,
                                "observers": observers_for(id),
                            });
                            let _ = out.send(WsMessage::Text(confirm.to_string().into())).await;
                        }
                    }
                });
            }
        });
    });
    rx.recv().unwrap()
}

fn echo() -> RegisterFunction {
    RegisterFunction::new(|input: Value| Ok::<_, Error>(input))
}

fn wait_for(what: &str, cond: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

struct Host {
    iii: IIIClient,
    plain: FunctionRef,
    ten_none: FunctionRef,
    one_match: FunctionRef,
}

fn host() -> Host {
    let address = spawn_fake_engine();
    let iii = register_worker(
        &format!("ws://{address}"),
        InitOptions {
            metadata: Some(WorkerMetadata {
                name: "bench".into(),
                ..WorkerMetadata::default()
            }),
            otel: Some(OtelConfig {
                enabled: Some(false),
                ..OtelConfig::default()
            }),
            identity: WorkerIdentityMode::Explicit,
            ..InitOptions::default()
        },
    );
    let plain = iii.register_function("bench::plain", echo());
    let ten_none = iii.register_function("bench::ten-none", echo());
    let one_match = iii.register_function("bench::one-match", echo());
    for function in [&plain, &ten_none, &one_match] {
        wait_for("local dispatch", || function.local_dispatch_active());
    }
    wait_for("the observers", || {
        ten_none.observers().len() == 10 && one_match.observers().len() == 1
    });
    Host {
        iii,
        plain,
        ten_none,
        one_match,
    }
}

/// Mean time of one `trigger()` of `function_id`, on a current-thread runtime.
fn bench_trigger(c: &mut Criterion, name: &str, iii: &IIIClient, function_id: &'static str) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    c.bench_function(name, |b| {
        b.iter_custom(|iters| {
            rt.block_on(async {
                let start = Instant::now();
                for i in 0..iters {
                    let out = iii
                        .trigger(TriggerRequest::new(function_id, json!({ "n": i })))
                        .await
                        .unwrap();
                    black_box(out);
                }
                start.elapsed()
            })
        })
    });
}

fn observe_benches(c: &mut Criterion) {
    let host = host();

    c.bench_function("is_observed/false", |b| {
        b.iter(|| black_box(&host.plain).is_observed())
    });
    assert!(!host.plain.is_observed());
    c.bench_function("is_observed/true", |b| {
        b.iter(|| black_box(&host.one_match).is_observed())
    });

    // What the echo handler costs by itself: the sync wrapper deserializes the
    // input and serializes the output.
    c.bench_function("handler_alone/echo", |b| {
        let mut i = 0u64;
        b.iter(|| {
            i += 1;
            let input: Value = serde_json::from_value(json!({ "n": i })).unwrap();
            black_box(serde_json::to_value(&input).unwrap())
        })
    });

    bench_trigger(c, "trigger_local/no_observers", &host.iii, "bench::plain");
    bench_trigger(
        c,
        "trigger_local/10_observers_none_matching",
        &host.iii,
        "bench::ten-none",
    );
    bench_trigger(
        c,
        "trigger_local/1_observer_matching",
        &host.iii,
        "bench::one-match",
    );
    let _ = (&host.ten_none, &host.one_match);
    host.iii.shutdown();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));
    targets = observe_benches
}
criterion_main!(benches);
