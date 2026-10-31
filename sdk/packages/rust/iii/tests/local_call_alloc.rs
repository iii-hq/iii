//! A worker's local call of its own private function passes the input to
//! the handler and the result back to the caller by move: what a call
//! allocates does not grow with its payload. The same holds when an observer
//! with `capture: []` and no filters matches the call: it is reported
//! without its payloads, and its input is not kept. A test binary of its
//! own, with an allocator that counts the allocations of each thread.

mod common;

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    time::{Duration, Instant},
};

use iii_helpers::observability::OtelConfig;
use iii_sdk::runtime::{FunctionRef, WorkerMetadata};
use iii_sdk::{
    CallRequest, Error, IIIClient, InitOptions, RegisterFunction, WorkerIdentityMode,
    register_worker,
};
use serde_json::{Value, json};

use common::mock_engine::MockEngine;

struct Counting;

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.with(|count| count.set(count.get() + 1));
        // SAFETY: forwarded unchanged to the system allocator.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded unchanged to the system allocator.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn allocations() -> usize {
    ALLOCATIONS.with(Cell::get)
}

const ROWS: usize = 1_000;

/// An event of `rows` rows, each an object with a string: a copy of it
/// allocates at least once per row.
fn event(rows: usize) -> Value {
    json!({
        "table": "orders",
        "rows": (0..rows)
            .map(|i| json!({ "id": i, "status": format!("paid-{i}") }))
            .collect::<Vec<_>>(),
    })
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
struct Event {
    table: String,
    rows: Vec<Row>,
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
struct Row {
    id: u64,
    status: String,
}

/// The fewest allocations one local call of `function_id` with `payload`
/// made on this thread, over a few calls.
async fn call_allocations(iii: &IIIClient, function_id: &str, payload: &Value) -> usize {
    let mut fewest = usize::MAX;
    for _ in 0..8 {
        let payload = payload.clone();
        let before = allocations();
        let out = iii.call(CallRequest::new(function_id, payload)).await;
        let made = allocations() - before;
        let out = out.expect("the local call");
        assert_eq!(out["table"], "orders");
        fewest = fewest.min(made);
    }
    fewest
}

async fn until(what: &str, cond: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn a_local_call_of_a_private_function_copies_nothing() {
    let mock = MockEngine::start().await;
    let iii = register_worker(
        mock.url(),
        InitOptions {
            metadata: Some(WorkerMetadata {
                name: "source".into(),
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
    mock.wait_for(
        |m| {
            m.iter()
                .any(|f| f["function_id"] == "engine::workers::register")
        },
        Duration::from_secs(5),
    )
    .await;
    mock.send_to_client(json!({
        "type": "workerregistered", "worker_id": "w-1",
        "features": ["observe.v1", "observe.v2", "local-dispatch.v1"]
    }));
    let echo = || RegisterFunction::new(|event: Value| Ok::<_, Error>(event)).private();
    let unobserved = iii.register_function("copy::unobserved", echo());
    let counted = iii.register_function("copy::counted", echo());
    // the control: a typed handler converts the event both ways
    let typed = iii.register_function(
        "copy::typed",
        RegisterFunction::new(|event: Event| Ok::<_, Error>(event)).private(),
    );
    for (function, observers) in [
        (&unobserved, json!([])),
        // counts calls: captures nothing, filters nothing
        (
            &counted,
            json!([{ "id": "count", "args": {}, "filters": {}, "target": "stats::count",
                     "capture": [], "on": "success" }]),
        ),
        (&typed, json!([])),
    ] {
        mock.wait_for(
            |m| {
                m.iter()
                    .any(|f| f["type"] == "registerfunction" && f["id"] == function.id)
            },
            Duration::from_secs(5),
        )
        .await;
        mock.send_to_client(json!({
            "type": "functionregistered", "id": function.id, "namespace": "default",
            "revision": 1, "local_dispatch": true, "observers": observers,
        }));
    }
    let ready = |function: &FunctionRef| function.local_dispatch_active();
    until("local dispatch", || {
        ready(&unobserved) && ready(&counted) && ready(&typed)
    })
    .await;
    until("the counting observer", || counted.is_observed()).await;

    let (small, large) = (event(1), event(ROWS));
    let grown = |small: usize, large: usize| large.saturating_sub(small);

    let control = grown(
        call_allocations(&iii, "copy::typed", &small).await,
        call_allocations(&iii, "copy::typed", &large).await,
    );
    assert!(
        control >= ROWS,
        "the counter sees a conversion: {control} more allocations for {ROWS} more rows"
    );
    for function_id in ["copy::unobserved", "copy::counted"] {
        let made = grown(
            call_allocations(&iii, function_id, &small).await,
            call_allocations(&iii, function_id, &large).await,
        );
        assert_eq!(
            made, 0,
            "{function_id}: {ROWS} more rows cost {made} more allocations"
        );
    }

    // the counting observer's reports carry ids, status and timing only
    let items = mock
        .wait_for(
            |m| {
                m.iter()
                    .filter(|f| f["type"] == "invocationsobserved")
                    .flat_map(|f| f["items"].as_array().cloned().unwrap_or_default())
                    .any(|item| item["function_id"] == "copy::counted")
            },
            Duration::from_secs(5),
        )
        .await
        .into_iter()
        .filter(|f| f["type"] == "invocationsobserved")
        .flat_map(|f| f["items"].as_array().cloned().unwrap_or_default())
        .collect::<Vec<_>>();
    let item = items
        .iter()
        .find(|item| item["function_id"] == "copy::counted")
        .expect("a report of copy::counted");
    assert_eq!(item["matched"], json!(["count"]));
    iii.shutdown_async().await;
}
