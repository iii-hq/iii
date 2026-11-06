//! Engine memory per observer at rest (memory experiment
//! `observer-footprint`): the heap bytes and blocks the engine holds for each
//! observer once it is active and nothing is in flight.
//!
//! Each scenario creates [`N`] observers in a fresh engine and reads a
//! counting global allocator before and after, once the engine settled:
//!
//! - who creates them: the engine (`observe_as_engine`) or a worker
//!   connection (`ObserverRegistry::observe`);
//! - where: each on its own function, or all on one function;
//! - with no filter, or with an `$eq` filter on a field of the input, with a
//!   value of its own (`{"input.key": {"$eq": "key-<i>"}}`), which the active
//!   set's filter index files.
//!
//! Every scenario targets one sink function. The functions are registered
//! before the first reading: what the engine holds for a registered
//! function nobody observes is not the observer's. The figures are the
//! median of three runs, after a run that warms up the engine's statics.
//! The live blocks are also counted by requested size, so the blocks each
//! observer adds show by size (`by size` lines): what the breakdown in the
//! evidence file maps to structures.
//!
//! `cargo test -p iii --test observe_memory_observer_footprint -- --test-threads=1 --nocapture`

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::{
        Arc,
        atomic::{AtomicIsize, Ordering},
    },
    time::Duration,
};

use iii::{
    engine::{Engine, EngineTrait, Handler, Outbound, RegisterFunctionRequest},
    function::FunctionResult,
    observer::Deliver,
    observer::{EngineObserver, ObserveRequest, ObserversConfig, engine_owned::no_args},
    protocol::{ObserveSource, ObserveTarget, ObserverState},
    worker_connections::WorkerConnection,
    workers::observability::metrics::ensure_default_meter,
};
use serde_json::{Value, json};
use tokio::sync::mpsc;

struct Counting;

static LIVE: AtomicIsize = AtomicIsize::new(0);
static BLOCKS: AtomicIsize = AtomicIsize::new(0);

/// Live blocks by requested size; the last slot counts every larger one.
const SIZES: usize = 4097;
static BY_SIZE: [AtomicIsize; SIZES] = [const { AtomicIsize::new(0) }; SIZES];
/// `BY_SIZE` at the first reading of a run, copied without allocating.
static BASE: [AtomicIsize; SIZES] = [const { AtomicIsize::new(0) }; SIZES];

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE.fetch_add(l.size() as isize, Ordering::Relaxed);
        BLOCKS.fetch_add(1, Ordering::Relaxed);
        BY_SIZE[l.size().min(SIZES - 1)].fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size() as isize, Ordering::Relaxed);
        BLOCKS.fetch_sub(1, Ordering::Relaxed);
        BY_SIZE[l.size().min(SIZES - 1)].fetch_sub(1, Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Observers per scenario.
const N: usize = 1024;
/// Measured runs per scenario (after one warm-up run).
const RUNS: usize = 3;

const SINK: &str = "mem::sink";

#[derive(Clone, Copy, Debug)]
enum Owner {
    Engine,
    Worker,
}

#[derive(Clone, Copy)]
struct Scenario {
    owner: Owner,
    /// Each observer on its own function, or all on one.
    distinct: bool,
    /// An indexed `$eq` filter, one value per observer.
    filtered: bool,
}

impl Scenario {
    fn name(&self) -> String {
        format!(
            "{:?}-owned, {}, {}",
            self.owner,
            if self.distinct {
                "each on its own function"
            } else {
                "all on one function"
            },
            if self.filtered {
                "$eq filter"
            } else {
                "no filter"
            }
        )
    }
}

/// What a run measured: the change in live bytes and blocks, and in live
/// blocks of each size.
struct Reading {
    bytes: isize,
    blocks: isize,
    by_size: Vec<(usize, isize)>,
}

fn register(engine: &Engine, id: &str) {
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: id.to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(|_input| async { FunctionResult::Success(None) }),
    );
}

fn function_of(i: usize, distinct: bool) -> String {
    format!("mem::observed-{}", if distinct { i } else { 0 })
}

fn filter_of(i: usize) -> Value {
    json!({ "input.key": { "$eq": format!("key-{i}") } })
}

fn id_of(i: usize) -> String {
    // The length of a UUID, as the SDKs make them.
    format!("obs-{i:032}")
}

/// A worker connection whose socket is drained, registered with a name.
fn worker(engine: &Engine) -> WorkerConnection {
    let (tx, mut rx) = mpsc::channel::<Outbound>(1 << 16);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let worker = WorkerConnection::new(tx);
    engine.worker_registry.register_worker(worker.clone());
    engine.worker_registry.update_worker_metadata(
        &worker.id,
        "node".to_string(),
        None,
        Some("mem-observer-worker".to_string()),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    worker
}

/// Lets the engine finish what it started: no work in flight, drained
/// sockets.
async fn settle() {
    for _ in 0..5 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn first_reading() -> (isize, isize) {
    let reading = (LIVE.load(Ordering::SeqCst), BLOCKS.load(Ordering::SeqCst));
    for (base, live) in BASE.iter().zip(&BY_SIZE) {
        base.store(live.load(Ordering::SeqCst), Ordering::SeqCst);
    }
    reading
}

async fn run(scenario: Scenario) -> Reading {
    let engine = Arc::new(Engine::new());
    engine.observers.set_config(ObserversConfig {
        max_per_connection: 1 << 20,
        max_per_function: 1 << 20,
        ..ObserversConfig::default()
    });
    register(&engine, SINK);
    for i in 0..if scenario.distinct { N } else { 1 } {
        register(&engine, &function_of(i, scenario.distinct));
    }
    let worker = matches!(scenario.owner, Owner::Worker).then(|| worker(&engine));
    settle().await;

    let (bytes, blocks) = first_reading();
    for i in 0..N {
        let function = function_of(i, scenario.distinct);
        let filters = scenario.filtered.then(|| filter_of(i));
        match &worker {
            None => engine
                .observers
                .observe_as_engine(EngineObserver {
                    id: id_of(i),
                    function: ("default".to_string(), function),
                    args: no_args(),
                    filters,
                    target: ("default".to_string(), SINK.to_string()),
                    deliver: Deliver::Input,
                    metadata: None,
                })
                .expect("observe as the engine"),
            Some(worker) => {
                engine
                    .observers
                    .observe(
                        &engine,
                        worker,
                        ObserveRequest {
                            id: id_of(i),
                            source: ObserveSource {
                                function_id: function,
                                namespace: None,
                                args: None,
                                filters,
                                capture: None,
                                on: None,
                                deliver: Some(serde_json::json!("input")),
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
                        },
                    )
                    .await
            }
        }
    }
    settle().await;
    for i in 0..N {
        let state = engine.observers.get(&id_of(i)).map(|o| o.state());
        assert_eq!(state, Some(ObserverState::Active), "observer {i} is active");
    }
    settle().await;

    let bytes = LIVE.load(Ordering::SeqCst) - bytes;
    let blocks = BLOCKS.load(Ordering::SeqCst) - blocks;
    let mut by_size = Vec::new();
    for (size, (base, live)) in BASE.iter().zip(&BY_SIZE).enumerate() {
        let added = live.load(Ordering::SeqCst) - base.load(Ordering::SeqCst);
        if added != 0 {
            by_size.push((size, added));
        }
    }
    drop(worker);
    drop(engine);
    Reading {
        bytes,
        blocks,
        by_size,
    }
}

fn median(mut values: Vec<isize>) -> isize {
    values.sort_unstable();
    values[values.len() / 2]
}

#[test]
fn observer_footprint_at_rest() {
    ensure_default_meter();
    let mut table = Vec::new();
    for owner in [Owner::Engine, Owner::Worker] {
        for distinct in [true, false] {
            for filtered in [false, true] {
                let scenario = Scenario {
                    owner,
                    distinct,
                    filtered,
                };
                let mut readings = Vec::new();
                for run_index in 0..=RUNS {
                    // A runtime per run: whatever a run spawned ends with it.
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("runtime");
                    let reading = runtime.block_on(run(scenario));
                    drop(runtime);
                    if run_index > 0 {
                        readings.push(reading);
                    }
                }
                let bytes = median(readings.iter().map(|r| r.bytes).collect());
                let blocks = median(readings.iter().map(|r| r.blocks).collect());
                println!(
                    "{}: {:.1} bytes, {:.2} blocks per observer (runs: {:?} bytes)",
                    scenario.name(),
                    bytes as f64 / N as f64,
                    blocks as f64 / N as f64,
                    readings.iter().map(|r| r.bytes).collect::<Vec<_>>(),
                );
                // The blocks of each size the last run added, at least one
                // per eight observers.
                let last = readings.last().expect("a run");
                let mut sizes: Vec<String> = last
                    .by_size
                    .iter()
                    .filter(|(_, added)| added.unsigned_abs() * 8 >= N)
                    .map(|(size, added)| {
                        format!(
                            "{}{size}B x{:.2}",
                            if *size == SIZES - 1 { ">=" } else { "" },
                            *added as f64 / N as f64
                        )
                    })
                    .collect();
                let listed: isize = last
                    .by_size
                    .iter()
                    .filter(|(_, added)| added.unsigned_abs() * 8 >= N)
                    .map(|(size, added)| *size as isize * added)
                    .sum();
                sizes.push(format!(
                    "rest {:.1}B",
                    (last.bytes - listed) as f64 / N as f64
                ));
                println!("  by size: {}", sizes.join(", "));
                table.push((scenario.name(), bytes, blocks));
            }
        }
    }
    println!();
    println!("| scenario | bytes / observer | blocks / observer |");
    println!("|---|---:|---:|");
    for (name, bytes, blocks) in &table {
        println!(
            "| {name} | {:.1} | {:.2} |",
            *bytes as f64 / N as f64,
            *blocks as f64 / N as f64
        );
    }
    for (name, bytes, _) in &table {
        assert!(*bytes > 0, "{name}: the observers hold memory");
    }
}
