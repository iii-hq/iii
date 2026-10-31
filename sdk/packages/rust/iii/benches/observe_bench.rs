//! observe hot paths on a Rust host: the rows of the observe v2 performance
//! budget (spec §13) that run in-process.
//!
//! - `is_observed()` with nobody observing: one atomic load, no allocation;
//! - a local `call()` with no observers (`call_local/no_observers`), and of a
//!   private function (`call_local/private`): the handler's own cost (compare
//!   with `handler_alone`), no copy of input or result (`tests/local_call_alloc.rs`
//!   counts the allocations);
//! - a local call of a private function with a deliver hook and no observers
//!   (`call_local/private_deliver_hook`, the shape of `http::on-request`
//!   with no routes), against the same function without the hook
//!   (`call_local/private_event`): the hook's `Call` carries the input and the
//!   handler's output as two values, so the input is copied once before the
//!   handler takes it;
//! - a local call with 10 observers, none matching: < 5 µs;
//! - a local call with 1 matching observer: 1 report item, batched;
//! - a local call of a function whose 1,000 observers each filter on their
//!   own `input.tenant` (`{ "$eq": "tenant-<i>" }`): one of them matching
//!   (`call_local/1000_eq_observers_1_matching`, 1 report item), and none
//!   (`call_local/1000_eq_observers_none_matching`);
//! - a function matched by keys whose 10,000 observers are filed under other
//!   keys: `is_observed_for(call)` is one hash lookup per call key, no
//!   allocation;
//! - matching by keys, 1 matching observer among 10,000;
//! - an event source (the handler returns its input) observed with input and
//!   output captured: each report item carries the event once.
//!
//! A local call needs the engine's `functionregistered`, so a small in-process
//! fake engine confirms the functions with the observers each case needs, and
//! counts the report items it receives.
//!
//! After the criterion rows, each `call_local/*` case runs again paced at
//! 5,000 calls a second for 2 seconds (`paced/5000/<case>`, the rate of the
//! spec's budget): the latency of each call as percentiles, and the report
//! items the fake engine received per call.
//!
//! `cargo bench -p iii-sdk --bench observe_bench`

use std::{
    hint::black_box,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use criterion::{Criterion, criterion_group, criterion_main};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use iii_helpers::observability::OtelConfig;
use iii_sdk::runtime::{FunctionRef, WorkerMetadata};
use iii_sdk::{
    Call, CallRequest, Error, IIIClient, InitOptions, Match, Observer, RegisterFunction,
    WorkerIdentityMode, register_worker,
};

/// Observers filed under keys no benchmark call has, plus one under `hit`.
const MATCH_KEY_OBSERVERS: usize = 10_000;

/// Observers of `bench::thousand-eq`, each filtering on its own tenant.
const EQ_OBSERVERS: usize = 1_000;

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
        "bench::match-keys" => Value::Array(
            (0..MATCH_KEY_OBSERVERS)
                .map(|i| {
                    let expression = if i == 0 {
                        "hit".to_string()
                    } else {
                        format!("expr-{i}")
                    };
                    json!({
                        "id": format!("match-key-{i}"), "args": { "expression": expression },
                        "target": "bench::target", "filters": {},
                        "capture": ["input"], "on": "success"
                    })
                })
                .collect(),
        ),
        "bench::thousand-eq" => Value::Array(
            (0..EQ_OBSERVERS)
                .map(|i| {
                    json!({
                        "id": format!("tenant-obs-{i:04}"), "args": {}, "target": "bench::target",
                        "filters": { "$and": [{ "input.tenant": { "$eq": format!("tenant-{i}") } }] },
                        "capture": ["input"], "on": "success"
                    })
                })
                .collect(),
        ),
        "bench::source" => json!([{
            "id": "obs-source", "args": {}, "target": "bench::target", "filters": {},
            "capture": ["input", "output"], "on": "success"
        }]),
        _ => json!([]),
    }
}

/// Report items the fake engine received, and how many of them carried the
/// value once (`output_is_input`).
#[derive(Clone, Default)]
struct Received {
    items: Arc<AtomicUsize>,
    once: Arc<AtomicUsize>,
}

/// Accepts the worker, advertises `observe.v2`, and confirms every function
/// it registers. Report frames are counted; other frames are dropped.
fn spawn_fake_engine(received: Received) -> SocketAddr {
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
                let received = received.clone();
                tokio::spawn(async move {
                    let Ok(ws) = tokio_tungstenite::accept_async(socket).await else {
                        return;
                    };
                    let (mut out, mut inbound) = ws.split();
                    let mut hello_sent = false;
                    while let Some(Ok(frame)) = inbound.next().await {
                        let WsMessage::Text(text) = frame else {
                            continue;
                        };
                        let Ok(message) = serde_json::from_str::<Value>(text.as_str()) else {
                            continue;
                        };
                        if !hello_sent && message["function_id"] == "engine::workers::register" {
                            hello_sent = true;
                            let hello = json!({
                                "type": "workerregistered", "worker_id": "bench-worker",
                                "features": ["observe.v1", "observe.v2", "local-dispatch.v1"]
                            });
                            let _ = out.send(WsMessage::Text(hello.to_string().into())).await;
                        }
                        if message["type"] == "registerfunction" {
                            let id = message["id"].as_str().unwrap_or_default();
                            let confirm = json!({
                                "type": "functionregistered", "id": id, "namespace": "default",
                                "revision": 1, "local_dispatch": true,
                                "observers": observers_for(id),
                            });
                            let _ = out.send(WsMessage::Text(confirm.to_string().into())).await;
                        }
                        if message["type"] == "invocationsobserved" {
                            for item in message["items"].as_array().into_iter().flatten() {
                                received.items.fetch_add(1, Ordering::Relaxed);
                                if item["output_is_input"] == true && item.get("output").is_none() {
                                    received.once.fetch_add(1, Ordering::Relaxed);
                                }
                            }
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
    let deadline = Instant::now() + Duration::from_secs(30);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

struct Host {
    iii: IIIClient,
    plain: FunctionRef,
    one_match: FunctionRef,
    match_keys: FunctionRef,
    received: Received,
}

fn host() -> Host {
    let received = Received::default();
    let address = spawn_fake_engine(received.clone());
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
    iii.on_observer_match_for(
        "bench::match-keys",
        Match::keys(
            |obs| obs.args_value()["expression"].as_str().into(),
            |call| call.input["expression"].as_str().into(),
        ),
    );
    let plain = iii.register_function("bench::plain", echo());
    let private = iii.register_function("bench::private", echo().private());
    let ten_none = iii.register_function("bench::ten-none", echo());
    let one_match = iii.register_function(
        "bench::one-match",
        RegisterFunction::new(|input: Value| Ok::<_, Error>(json!({ "seen": input["n"] }))),
    );
    let match_keys = iii.register_function("bench::match-keys", echo().private());
    let thousand_eq = iii.register_function("bench::thousand-eq", echo());
    let source = iii.register_function("bench::source", echo().private());
    let event = iii.register_function("bench::private-event", echo().private());
    iii.on_observer_deliver_for(
        "bench::private-deliver",
        |call: Call, _observers: Vec<Observer>| async move { Ok(call.output.unwrap_or_default()) },
    );
    let delivered = iii.register_function("bench::private-deliver", echo().private());
    for function in [
        &plain,
        &private,
        &ten_none,
        &one_match,
        &match_keys,
        &thousand_eq,
        &source,
        &event,
        &delivered,
    ] {
        wait_for("local dispatch", || function.local_dispatch_active());
    }
    wait_for("the observers", || {
        ten_none.observers().len() == 10
            && one_match.observers().len() == 1
            && match_keys.observers().len() == MATCH_KEY_OBSERVERS
            && thousand_eq.observers().len() == EQ_OBSERVERS
            && source.observers().len() == 1
    });
    Host {
        iii,
        plain,
        one_match,
        match_keys,
        received,
    }
}

/// Mean time of one `call()` of `function_id` with `input(i)`, on a
/// current-thread runtime.
fn bench_call(
    c: &mut Criterion,
    name: &str,
    iii: &IIIClient,
    function_id: &'static str,
    input: impl Fn(u64) -> Value,
) {
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
                        .call(CallRequest::new(function_id, input(i)))
                        .await
                        .unwrap();
                    black_box(out);
                }
                start.elapsed()
            })
        })
    });
}

/// Calls per second of the paced pass, and its length.
const PACED_RATE: u64 = 5_000;
const PACED_SECS: u64 = 2;

/// Calls `function_id` with `input(i)` at `PACED_RATE` a second for
/// `PACED_SECS`, one at a time on a current-thread runtime, and prints the
/// latency of each call as percentiles and the report items the fake engine
/// received per call.
fn paced(host: &Host, name: &str, function_id: &'static str, input: impl Fn(u64) -> Value) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let calls = PACED_RATE * PACED_SECS;
    let period_ns = 1_000_000_000 / PACED_RATE;
    let items_before = host.received.items.load(Ordering::Relaxed);
    let once_before = host.received.once.load(Ordering::Relaxed);
    let mut latencies: Vec<u64> = Vec::with_capacity(calls as usize);
    let elapsed = rt.block_on(async {
        let start = Instant::now();
        for i in 0..calls {
            let due = start + Duration::from_nanos(period_ns * i);
            while Instant::now() < due {
                tokio::task::yield_now().await;
            }
            let sent = Instant::now();
            let out = host
                .iii
                .call(CallRequest::new(function_id, input(i)))
                .await
                .unwrap();
            latencies.push(sent.elapsed().as_nanos() as u64);
            black_box(out);
        }
        start.elapsed()
    });
    // Reports are batched: wait until the count stops growing.
    let mut items = host.received.items.load(Ordering::Relaxed);
    loop {
        std::thread::sleep(Duration::from_millis(250));
        let now = host.received.items.load(Ordering::Relaxed);
        if now == items {
            break;
        }
        items = now;
    }
    let once = host.received.once.load(Ordering::Relaxed) - once_before;
    latencies.sort_unstable();
    let micros = |ns: u64| ns as f64 / 1_000.0;
    let at = |p: f64| micros(latencies[((latencies.len() - 1) as f64 * p) as usize]);
    let mean = micros(latencies.iter().sum::<u64>()) / latencies.len() as f64;
    eprintln!(
        "paced/{PACED_RATE}/{name}: {calls} calls in {:.3} s ({:.0}/s); latency µs: mean {mean:.3} p50 {:.3} p90 {:.3} p99 {:.3} max {:.3}; report items per call {:.3}, carrying the value once {:.3}",
        elapsed.as_secs_f64(),
        calls as f64 / elapsed.as_secs_f64(),
        at(0.50),
        at(0.90),
        at(0.99),
        micros(*latencies.last().unwrap()),
        (items - items_before) as f64 / calls as f64,
        once as f64 / calls as f64,
    );
}

fn observe_benches(c: &mut Criterion) {
    let host = host();
    let numbered = |i: u64| json!({ "n": i });
    // One of the 1,000 tenants a call, in turn; and a tenant none observes.
    let tenant =
        |i: u64| json!({ "tenant": format!("tenant-{}", i % EQ_OBSERVERS as u64), "n": i });
    let no_tenant = |i: u64| json!({ "tenant": "tenant-none", "n": i });

    c.bench_function("is_observed/false", |b| {
        b.iter(|| black_box(&host.plain).is_observed())
    });
    assert!(!host.plain.is_observed());
    c.bench_function("is_observed/true", |b| {
        b.iter(|| black_box(&host.one_match).is_observed())
    });

    let miss = Call {
        input: json!({ "expression": "never-filed" }),
        ..Call::default()
    };
    assert!(!host.match_keys.is_observed_for(&miss));
    c.bench_function("is_observed_for/key_miss_of_10000", |b| {
        b.iter(|| black_box(&host.match_keys).is_observed_for(black_box(&miss)))
    });

    // What the echo handler costs by itself, with its input built as each
    // call builds it: a `Value` handler gets its input and returns its
    // output as they are.
    c.bench_function("handler_alone/echo", |b| {
        let echo = |input: Value| Ok::<_, Error>(input);
        let mut i = 0u64;
        b.iter(|| {
            i += 1;
            black_box(echo(black_box(json!({ "n": i }))).unwrap())
        })
    });

    bench_call(
        c,
        "call_local/no_observers",
        &host.iii,
        "bench::plain",
        numbered,
    );
    bench_call(
        c,
        "call_local/private",
        &host.iii,
        "bench::private",
        numbered,
    );
    bench_call(
        c,
        "call_local/10_observers_none_matching",
        &host.iii,
        "bench::ten-none",
        numbered,
    );
    bench_call(
        c,
        "call_local/1_observer_matching",
        &host.iii,
        "bench::one-match",
        numbered,
    );
    bench_call(
        c,
        "call_local/match_keys_1_of_10000",
        &host.iii,
        "bench::match-keys",
        |i| json!({ "expression": "hit", "n": i }),
    );
    bench_call(
        c,
        "call_local/1000_eq_observers_1_matching",
        &host.iii,
        "bench::thousand-eq",
        tenant,
    );
    bench_call(
        c,
        "call_local/1000_eq_observers_none_matching",
        &host.iii,
        "bench::thousand-eq",
        no_tenant,
    );
    let event = |i: u64| json!({ "table": "invoices", "op": "update", "row": { "id": i, "status": "paid" } });
    bench_call(
        c,
        "call_local/private_event",
        &host.iii,
        "bench::private-event",
        event,
    );
    bench_call(
        c,
        "call_local/private_deliver_hook",
        &host.iii,
        "bench::private-deliver",
        event,
    );
    let before = host.received.once.load(Ordering::Relaxed);
    // Set when the case runs: a `cargo bench` filter can leave it out.
    let source_ran = std::cell::Cell::new(false);
    bench_call(
        c,
        "call_local/event_source_observed",
        &host.iii,
        "bench::source",
        |i| {
            source_ran.set(true);
            event(i)
        },
    );
    // the reports of the source carried each event once
    if source_ran.get() {
        wait_for("the source's reports", || {
            host.received.once.load(Ordering::Relaxed) > before
        });
    }
    eprintln!(
        "fake engine: {} report items, {} carrying the value once (output_is_input)",
        host.received.items.load(Ordering::Relaxed),
        host.received.once.load(Ordering::Relaxed)
    );

    paced(&host, "call_local/no_observers", "bench::plain", numbered);
    paced(&host, "call_local/private", "bench::private", numbered);
    paced(
        &host,
        "call_local/10_observers_none_matching",
        "bench::ten-none",
        numbered,
    );
    paced(
        &host,
        "call_local/1_observer_matching",
        "bench::one-match",
        numbered,
    );
    paced(
        &host,
        "call_local/match_keys_1_of_10000",
        "bench::match-keys",
        |i| json!({ "expression": "hit", "n": i }),
    );
    paced(
        &host,
        "call_local/1000_eq_observers_1_matching",
        "bench::thousand-eq",
        tenant,
    );
    paced(
        &host,
        "call_local/1000_eq_observers_none_matching",
        "bench::thousand-eq",
        no_tenant,
    );
    paced(
        &host,
        "call_local/private_event",
        "bench::private-event",
        event,
    );
    paced(
        &host,
        "call_local/private_deliver_hook",
        "bench::private-deliver",
        event,
    );
    paced(
        &host,
        "call_local/event_source_observed",
        "bench::source",
        event,
    );
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
