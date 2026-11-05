// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Bytes and allocations the engine spends on each observed call besides its
//! payloads (memory experiment `event-fields`).
//!
//! A counting global allocator (this test binary only) reads the heap around
//! in-process scenarios: an in-process engine, the observed function and the
//! observer's target registered in-process, small payloads (input
//! `{ "n": i }`, output `{ "ok": true }`), the default capture (`input`,
//! `output`, `caller`) and the envelope delivered. Each figure is the median
//! of three runs, each on a fresh engine, read once the engine has settled:
//! nothing in flight but targets that never answer.
//!
//! - `unobserved`: `N` calls of the function with no observer, the control.
//! - `queued`: the observer's target never answers and `max_in_flight` is 1,
//!   so the events of the `N` calls stay queued. Live bytes and blocks per
//!   queued event, allocations made per call.
//! - `running`: the same with `max_in_flight` high enough for every
//!   activation to run, its target holding its argument. Live bytes and
//!   blocks per running activation: its event, its argument, its task.
//! - `answered`: the target answers at once. Allocations made per call and
//!   activation, and what stays live once every activation ran.
//! - `emitted`: `N` events of an engine-hosted source, queued as in
//!   `queued`.
//!
//! `cargo test -p iii --test observe_memory_event_fields -- --test-threads=1 --nocapture`

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::{
        Arc,
        atomic::{AtomicIsize, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use iii::engine::{Engine, EngineTrait, Handler, Outbound, RegisterFunctionRequest};
use iii::function::FunctionResult;
use iii::observer::{ObserveRequest, ObserversConfig, events::register_event_source};
use iii::protocol::{DEFAULT_NAMESPACE, Deliver, ObserveSource, ObserveTarget, ObserverState};
use iii::worker_connections::WorkerConnection;
use iii::workers::observability::metrics::ensure_default_meter;
use serde_json::{Value, json};
use tokio::sync::{Semaphore, mpsc};

struct Counting;

/// Bytes allocated and not freed.
static LIVE: AtomicIsize = AtomicIsize::new(0);
/// Blocks allocated and not freed.
static BLOCKS: AtomicIsize = AtomicIsize::new(0);
/// Blocks allocated, freed or not.
static MADE: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE.fetch_add(l.size() as isize, Ordering::Relaxed);
        BLOCKS.fetch_add(1, Ordering::Relaxed);
        MADE.fetch_add(1, Ordering::Relaxed);
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
const EVENT_SOURCE: &str = "mem::event";
const SINK: &str = "mem::sink";
const OBSERVER: &str = "mem-obs";

/// Calls made before the counters are read: the first activation, the
/// evaluator tasks and every cache the path warms.
const WARM: usize = 64;

/// Calls measured in a run.
const N: usize = 1_000;

const RUNS: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Scenario {
    Unobserved,
    Queued,
    Running,
    Answered,
    Emitted,
}

impl Scenario {
    fn name(self) -> &'static str {
        match self {
            Self::Unobserved => "unobserved",
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Answered => "answered",
            Self::Emitted => "emitted",
        }
    }
}

#[derive(Clone, Copy)]
struct Heap {
    live: isize,
    blocks: isize,
    made: usize,
}

impl Heap {
    fn now() -> Self {
        Self {
            live: LIVE.load(Ordering::SeqCst),
            blocks: BLOCKS.load(Ordering::SeqCst),
            made: MADE.load(Ordering::SeqCst),
        }
    }

    /// What each of `n` units cost since `before`.
    fn per(self, before: Heap, n: usize) -> Cost {
        let n = n as f64;
        Cost {
            bytes: (self.live - before.live) as f64 / n,
            blocks: (self.blocks - before.blocks) as f64 / n,
            made: (self.made - before.made) as f64 / n,
        }
    }
}

/// Per unit: bytes and blocks still live, allocations made.
#[derive(Clone, Copy, Debug)]
struct Cost {
    bytes: f64,
    blocks: f64,
    made: f64,
}

fn median(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    samples[samples.len() / 2]
}

fn median_cost(costs: &[Cost]) -> Cost {
    Cost {
        bytes: median(costs.iter().map(|c| c.bytes).collect()),
        blocks: median(costs.iter().map(|c| c.blocks).collect()),
        made: median(costs.iter().map(|c| c.made).collect()),
    }
}

fn registration(function_id: &str) -> RegisterFunctionRequest {
    RegisterFunctionRequest {
        function_id: function_id.to_string(),
        description: None,
        request_format: None,
        response_format: None,
        metadata: None,
    }
}

/// A worker connection whose socket is drained: the observing worker.
fn worker(engine: &Engine) -> WorkerConnection {
    let (tx, mut rx) = mpsc::channel::<Outbound>(1 << 16);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let worker = WorkerConnection::new(tx);
    engine.worker_registry.register_worker(worker.clone());
    worker
}

async fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(300);
    while !done() {
        assert!(Instant::now() < deadline, "{what}: not in time");
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

/// Lets every task that can run finish.
async fn settle() {
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
}

struct Run {
    engine: Arc<Engine>,
    _worker: Option<WorkerConnection>,
    /// Activations the target started.
    entered: Arc<AtomicUsize>,
    scenario: Scenario,
}

impl Run {
    async fn new(scenario: Scenario) -> Self {
        let engine = Arc::new(Engine::new());
        engine.observers.set_config(ObserversConfig {
            max_queue_per_observer: 4 * (WARM + N),
            activation_timeout_ms: 3_600_000,
            ..ObserversConfig::default()
        });
        engine.register_function_handler(
            registration(SOURCE),
            Handler::new(|_input: Value| async {
                FunctionResult::Success(Some(json!({ "ok": true })))
            }),
        );
        register_event_source(&engine, EVENT_SOURCE, "memory test events", json!({}));
        let gate = Arc::new(Semaphore::new(if scenario == Scenario::Answered {
            Semaphore::MAX_PERMITS
        } else {
            0
        }));
        let entered = Arc::new(AtomicUsize::new(0));
        {
            let (gate, entered) = (gate.clone(), entered.clone());
            engine.register_function_handler(
                registration(SINK),
                Handler::new(move |input: Value| {
                    let (gate, entered) = (gate.clone(), entered.clone());
                    async move {
                        entered.fetch_add(1, Ordering::AcqRel);
                        // Held while the target runs, as a target holds it.
                        let _input = input;
                        let _permit = gate.acquire().await;
                        FunctionResult::Success(None)
                    }
                }),
            );
        }
        let observing = (scenario != Scenario::Unobserved).then(|| worker(&engine));
        if let Some(worker) = &observing {
            let max_in_flight = match scenario {
                Scenario::Running => (WARM + N) as u32,
                _ => 1,
            };
            let source = match scenario {
                Scenario::Emitted => EVENT_SOURCE,
                _ => SOURCE,
            };
            let _ = engine
                .observers
                .observe(
                    &engine,
                    worker,
                    ObserveRequest {
                        id: OBSERVER.to_string(),
                        source: ObserveSource {
                            function_id: source.to_string(),
                            namespace: None,
                            args: None,
                            filters: None,
                            capture: None,
                            on: None,
                            deliver: Some(Deliver::Envelope),
                        },
                        target: ObserveTarget {
                            function_id: SINK.to_string(),
                            namespace: None,
                        },
                        metadata: None,
                        max_fires: None,
                        expires_in_ms: None,
                        max_in_flight: Some(max_in_flight),
                    },
                )
                .await;
            let observers = engine.observers.clone();
            until("the observer is active", || {
                observers.get(OBSERVER).map(|o| o.state()) == Some(ObserverState::Active)
            })
            .await;
        }
        Self {
            engine,
            _worker: observing,
            entered,
            scenario,
        }
    }

    /// Calls `count` more times (or emits `count` more events).
    async fn fire(&self, from: usize, count: usize) {
        for i in from..from + count {
            match self.scenario {
                Scenario::Emitted => {
                    self.engine
                        .observers
                        .emit(DEFAULT_NAMESPACE, EVENT_SOURCE, || Some(json!({ "n": i })))
                }
                _ => {
                    let answer = self.engine.call(SOURCE, json!({ "n": i })).await;
                    assert!(answer.is_ok(), "{answer:?}");
                }
            }
        }
    }

    /// Waits until the `total` calls so far reached where the scenario
    /// keeps them, then for the engine to settle.
    async fn reached(&self, total: usize) {
        let observers = self.engine.observers.clone();
        let entered = self.entered.clone();
        match self.scenario {
            Scenario::Unobserved => {}
            Scenario::Queued | Scenario::Emitted => {
                until("every event queued", || {
                    observers.evaluated() == total as u64
                        && observers.in_flight(OBSERVER) == Some((1, total - 1))
                })
                .await
            }
            Scenario::Running => {
                until("every activation running", || {
                    entered.load(Ordering::Acquire) == total
                })
                .await
            }
            Scenario::Answered => {
                until("every activation answered", || {
                    entered.load(Ordering::Acquire) == total
                        && observers.in_flight(OBSERVER) == Some((0, 0))
                })
                .await
            }
        }
        settle().await;
    }

    /// What each of `N` measured calls costs, after `WARM` unmeasured ones.
    async fn measure(&self) -> Cost {
        self.fire(0, WARM).await;
        self.reached(WARM).await;
        let before = Heap::now();
        self.fire(WARM, N).await;
        self.reached(WARM + N).await;
        Heap::now().per(before, N)
    }
}

async fn cost_of(scenario: Scenario) -> Cost {
    let mut costs = Vec::new();
    for _ in 0..RUNS {
        let run = Run::new(scenario).await;
        costs.push(run.measure().await);
    }
    median_cost(&costs)
}

#[tokio::test]
async fn bytes_and_allocations_per_observed_call() {
    ensure_default_meter();
    let scenarios = [
        Scenario::Unobserved,
        Scenario::Queued,
        Scenario::Running,
        Scenario::Answered,
        Scenario::Emitted,
    ];
    let mut costs = Vec::new();
    for scenario in scenarios {
        costs.push((scenario, cost_of(scenario).await));
    }
    let control = costs[0].1;
    println!();
    println!(
        "per call (median of {RUNS} runs of {N} calls; `net`: minus `unobserved`; live = still \
         allocated once settled)"
    );
    println!(
        "{:<12} {:>12} {:>12} {:>12} {:>12} {:>12}",
        "scenario", "live bytes", "live blocks", "allocs", "net bytes", "net allocs"
    );
    for (scenario, cost) in &costs {
        let (net_bytes, net_made) = match scenario {
            Scenario::Unobserved | Scenario::Emitted => (cost.bytes, cost.made),
            _ => (cost.bytes - control.bytes, cost.made - control.made),
        };
        println!(
            "{:<12} {:>12.1} {:>12.2} {:>12.2} {:>12.1} {:>12.2}",
            scenario.name(),
            cost.bytes,
            cost.blocks,
            cost.made,
            net_bytes,
            net_made
        );
    }
    let queued = costs[1].1;
    assert!(queued.bytes > 0.0, "a queued event holds something");
}
