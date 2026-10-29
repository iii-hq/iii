//! The fast paths a source asks before building an event: `is_observed()` is
//! one atomic load, and `is_observed_for(&call)` on a function delivered by
//! [`Keyed`] one hash lookup per call key; neither allocates. A test binary of
//! its own, with an allocator that counts the allocations of each thread.

mod common;

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    time::{Duration, Instant},
};

use iii_helpers::observability::OtelConfig;
use iii_sdk::runtime::WorkerMetadata;
use iii_sdk::{
    Call, Error, IIIClient, InitOptions, Keyed, RegisterFunction, WorkerIdentityMode,
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

/// Allocations `f` made on this thread, with a live counter as the control.
fn allocations_of(f: impl FnOnce()) -> usize {
    let before = allocations();
    f();
    let made = allocations() - before;

    let before = allocations();
    let boxed = std::hint::black_box(Box::new(42u64));
    assert!(allocations() > before, "the counter is live");
    drop(boxed);
    made
}

#[test]
fn is_observed_does_not_allocate() {
    let iii = IIIClient::new("ws://127.0.0.1:1");
    let function = iii.register_function(
        "alloc::f",
        RegisterFunction::new(|input: Value| Ok::<_, Error>(input)),
    );
    let mut observed = 0;
    let made = allocations_of(|| {
        for _ in 0..10_000 {
            if std::hint::black_box(&function).is_observed() {
                observed += 1;
            }
        }
    });
    assert_eq!(observed, 0);
    assert_eq!(made, 0, "is_observed() allocated");
}

#[tokio::test]
async fn a_keyed_miss_does_not_allocate() {
    let mock = MockEngine::start().await;
    let iii = register_worker(
        mock.url(),
        InitOptions {
            metadata: Some(WorkerMetadata {
                name: "alloc".into(),
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
    iii.on_observer_deliver_for(
        "cron::on-tick",
        Keyed::new(
            |obs| obs.args_value()["expression"].as_str().into(),
            |call| call.input["expression"].as_str().into(),
        ),
    );
    let on_tick = iii.register_function(
        "cron::on-tick",
        RegisterFunction::new(|tick: Value| Ok::<_, Error>(tick)).private(),
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
    mock.wait_for(
        |m| m.iter().any(|f| f["type"] == "registerfunction"),
        Duration::from_secs(5),
    )
    .await;
    mock.send_to_client(json!({
        "type": "functionregistered", "id": "cron::on-tick", "namespace": "default",
        "revision": 1, "local_dispatch": true,
        "observers": [{ "id": "nine", "args": { "expression": "0 0 9 * * * *" },
                        "filters": {}, "target": "reports::daily", "capture": ["input"],
                        "on": "success" }]
    }));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !on_tick.is_observed() {
        assert!(
            Instant::now() < deadline,
            "the observer never became active"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    let filed = Call {
        input: json!({ "expression": "0 0 9 * * * *" }),
        ..Call::default()
    };
    let other = Call {
        input: json!({ "expression": "0 */5 * * * * *" }),
        ..Call::default()
    };
    assert!(on_tick.is_observed_for(&filed), "the filed key is observed");
    let mut observed = 0;
    let made = allocations_of(|| {
        for _ in 0..10_000 {
            if std::hint::black_box(&on_tick).is_observed_for(&other) {
                observed += 1;
            }
        }
    });
    assert_eq!(observed, 0);
    assert_eq!(made, 0, "is_observed_for() allocated on a keyed miss");
    iii.shutdown_async().await;
}
