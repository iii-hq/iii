//! Memory held by the activations of observers whose target runs in the
//! engine process: the `Value` path (`project()` in `observer/dispatch.rs`).
//!
//! Each scenario registers a source function and `n` engine-owned observers
//! of it, all targeting one function that holds its argument until it is
//! released. One call of the source activates every observer; once every
//! target holds its argument and the engine has settled, the live heap above
//! the settled baseline is what the activations hold in flight.
//!
//! - `inprocess_handler_targets`: the target is a `Handler::new(|input:
//!   Value| ...)`, which owns its argument.
//! - `http_function_targets`: the target is an HTTP-invocation function; a
//!   local HTTP server reads each request whole, then holds its response.
//!   Its figures include what the test's own server and the connections
//!   hold per request.
//!
//! Figures are the median of 3 rounds, each after a warm-up round.
//!
//! `cargo test -p iii --test observe_memory_inprocess_shared -- --test-threads=1 --nocapture`

use std::{
    alloc::{GlobalAlloc, Layout, System},
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicIsize, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use iii::engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest};
use iii::function::FunctionResult;
use iii::invocation::{http_function::HttpFunctionConfig, method::HttpMethod};
use iii::observer::EngineObserver;
use iii::protocol::{DEFAULT_NAMESPACE, Deliver, ObserverState};
use iii::workers::http_functions::HttpFunctionsWorker;
use iii::workers::observability::metrics::ensure_default_meter;
use iii::workers::traits::Worker;
use serde_json::{Value, json};
use tokio::sync::Semaphore;

struct Counting;
static LIVE: AtomicIsize = AtomicIsize::new(0);
static BLOCKS: AtomicIsize = AtomicIsize::new(0);
/// Allocations made, never decremented: what reaching a state cost in calls
/// to the allocator.
static ALLOCS: AtomicIsize = AtomicIsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE.fetch_add(l.size() as isize, Ordering::Relaxed);
        BLOCKS.fetch_add(1, Ordering::Relaxed);
        ALLOCS.fetch_add(1, Ordering::Relaxed);
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

const SOURCE: &str = "mem::source";
const TARGET: &str = "mem::target";
const SIZES: [usize; 2] = [4 * 1024, 64 * 1024];
const OBSERVERS: [usize; 3] = [1, 4, 16];
const ROUNDS: usize = 3;

/// `(live bytes, live blocks, allocations made)`.
fn counters() -> (isize, isize, isize) {
    (
        LIVE.load(Ordering::SeqCst),
        BLOCKS.load(Ordering::SeqCst),
        ALLOCS.load(Ordering::SeqCst),
    )
}

/// Lets the engine finish what it started (no work in flight).
async fn settle() {
    tokio::time::sleep(Duration::from_millis(100)).await;
}

async fn wait_for(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

/// An object of small records whose JSON text is about `bytes` long: the
/// shape of a typical call payload, many small nodes.
fn payload(bytes: usize) -> Value {
    let mut items = Vec::new();
    let mut len = r#"{"items":[]}"#.len();
    let mut i = 0u32;
    while len < bytes {
        let item = json!({
            "id": i,
            "name": format!("item-{i:06}"),
            "tags": ["alpha", "beta"],
            "score": f64::from(i) * 1.5,
            "active": i.is_multiple_of(2),
        });
        len += serde_json::to_string(&item).expect("encodes").len() + 1;
        items.push(item);
        i += 1;
    }
    json!({ "items": items })
}

/// The heap one copy of `value` holds.
fn heap_of(value: &Value) -> isize {
    let before = LIVE.load(Ordering::SeqCst);
    let copy = value.clone();
    let held = LIVE.load(Ordering::SeqCst) - before;
    drop(copy);
    held
}

/// Counts the targets holding an argument, and releases them.
struct Gate {
    entered: AtomicUsize,
    left: AtomicUsize,
    release: Semaphore,
}

impl Gate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: AtomicUsize::new(0),
            left: AtomicUsize::new(0),
            release: Semaphore::new(0),
        })
    }

    async fn hold(&self) {
        self.entered.fetch_add(1, Ordering::SeqCst);
        self.release.acquire().await.expect("gate open").forget();
        self.left.fetch_add(1, Ordering::SeqCst);
    }
}

fn request(function_id: &str) -> RegisterFunctionRequest {
    RegisterFunctionRequest {
        function_id: function_id.to_string(),
        description: None,
        request_format: None,
        response_format: None,
        metadata: None,
    }
}

#[derive(Clone, Copy)]
enum Target {
    /// A `Handler::new(|input: Value| ...)` holding its input.
    Handler,
    /// An HTTP-invocation function, whose endpoint holds the response.
    Http,
}

/// An engine with the source function and `target`, which `gate` holds.
async fn setup(target: Target, gate: &Arc<Gate>) -> Arc<Engine> {
    let engine = Arc::new(Engine::new());
    engine.register_function_handler(
        request(SOURCE),
        Handler::new(|_input: Value| async { FunctionResult::Success(None) }),
    );
    match target {
        Target::Handler => {
            let gate = gate.clone();
            engine.register_function_handler(
                request(TARGET),
                Handler::new(move |input: Value| {
                    let gate = gate.clone();
                    async move {
                        gate.hold().await;
                        drop(input);
                        FunctionResult::Success(None)
                    }
                }),
            );
        }
        Target::Http => {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
            let url = format!("http://{}/hook", listener.local_addr().expect("addr"));
            let gate = gate.clone();
            let app = axum::Router::new().route(
                "/hook",
                axum::routing::post(move |body: axum::body::Bytes| {
                    let gate = gate.clone();
                    async move {
                        drop(body);
                        gate.hold().await;
                        axum::http::StatusCode::OK
                    }
                }),
            );
            tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
            let worker = HttpFunctionsWorker::create(
                engine.clone(),
                Some(json!({ "security": {
                    "url_allowlist": ["*"],
                    "block_private_ips": false,
                    "require_https": false,
                } })),
            )
            .await
            .expect("http functions worker");
            worker.initialize().await.expect("initialize");
            let http = engine
                .service_registry
                .get_service::<HttpFunctionsWorker>("http_functions")
                .expect("http functions service");
            http.register_http_function(
                DEFAULT_NAMESPACE,
                HttpFunctionConfig {
                    function_path: TARGET.to_string(),
                    url,
                    method: HttpMethod::Post,
                    timeout_ms: Some(60_000),
                    headers: HashMap::new(),
                    auth: None,
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                    registered_at: None,
                    updated_at: None,
                },
            )
            .await
            .expect("register the http function");
        }
    }
    engine
}

/// Adds observers `from..to` of the source, targeting the target.
async fn add_observers(engine: &Arc<Engine>, from: usize, to: usize, deliver: &Deliver) {
    for i in from..to {
        let id = format!("mem-{i}");
        engine
            .observers
            .observe_as_engine(EngineObserver {
                id: id.clone(),
                function: (DEFAULT_NAMESPACE.to_string(), SOURCE.to_string()),
                args: json!({}),
                filters: None,
                target: (DEFAULT_NAMESPACE.to_string(), TARGET.to_string()),
                deliver: deliver.clone(),
                metadata: None,
            })
            .expect("observe");
        wait_for("an active observer", || {
            engine.observers.get(&id).map(|o| o.state()) == Some(ObserverState::Active)
        })
        .await;
    }
}

struct Sample {
    bytes: isize,
    blocks: isize,
    allocs: isize,
}

/// One call of the source with `input`, measured once its `n` activations
/// all hold their argument; then they are released.
async fn round(engine: &Arc<Engine>, gate: &Gate, n: usize, input: &Value) -> Sample {
    gate.entered.store(0, Ordering::SeqCst);
    gate.left.store(0, Ordering::SeqCst);
    settle().await;
    let (bytes, blocks, allocs) = counters();
    engine.call(SOURCE, input).await.expect("source call");
    wait_for("the targets to hold", || {
        gate.entered.load(Ordering::SeqCst) == n
    })
    .await;
    settle().await;
    let (held_bytes, held_blocks, made) = counters();
    gate.release.add_permits(n);
    wait_for("the targets to return", || {
        gate.left.load(Ordering::SeqCst) == n
    })
    .await;
    Sample {
        bytes: held_bytes - bytes,
        blocks: held_blocks - blocks,
        allocs: made - allocs,
    }
}

fn median(mut samples: Vec<isize>) -> isize {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

async fn measure(name: &str, target: Target) {
    ensure_default_meter();
    let inputs: Vec<(usize, Value)> = SIZES.iter().map(|&size| (size, payload(size))).collect();
    for (size, input) in &inputs {
        println!(
            "payload {size} B target: JSON {} B, one Value copy {} B on the heap",
            serde_json::to_string(input).expect("encodes").len(),
            heap_of(input)
        );
    }
    println!(
        "{:<8} {:<8} {:>6} {:>3} | {:>10} {:>8} | {:>10} {:>8} | {:>9}",
        "target", "deliver", "size", "n", "held B", "blocks", "B/activ.", "blk/act.", "allocs"
    );
    for deliver in [Deliver::Envelope, Deliver::Input] {
        let gate = Gate::new();
        let engine = setup(target, &gate).await;
        let mut observers = 0;
        for n in OBSERVERS {
            add_observers(&engine, observers, n, &deliver).await;
            observers = n;
            for (size, input) in &inputs {
                round(&engine, &gate, n, input).await;
                let mut samples = Vec::new();
                for _ in 0..ROUNDS {
                    samples.push(round(&engine, &gate, n, input).await);
                }
                let bytes = median(samples.iter().map(|s| s.bytes).collect());
                let blocks = median(samples.iter().map(|s| s.blocks).collect());
                let allocs = median(samples.iter().map(|s| s.allocs).collect());
                println!(
                    "{:<8} {:<8} {:>6} {:>3} | {:>10} {:>8} | {:>10} {:>8} | {:>9}",
                    name,
                    deliver.as_str(),
                    size,
                    n,
                    bytes,
                    blocks,
                    bytes / n as isize,
                    blocks / n as isize,
                    allocs
                );
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn inprocess_handler_targets() {
    measure("handler", Target::Handler).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn http_function_targets() {
    measure("http", Target::Http).await;
}
