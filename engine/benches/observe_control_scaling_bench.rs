//! How the cost of one observer change grows with how many observers and
//! observed functions the engine already holds.
//!
//! - `observe`: the time one more `observe` takes (until the observer is
//!   active) once `n` observers exist, each on its own function
//!   (`distinct`) or all on one function (`one function`, the control).
//! - `disconnect`: the time `connection_closed` takes for a worker
//!   connection that created `n` observers, spread the same two ways.
//!
//! Prints a table; no criterion: each figure is the median of a few runs.
//!
//! `cargo bench -p iii --bench observe_control_scaling_bench`

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use iii::engine::{Engine, EngineTrait, Handler, Outbound, RegisterFunctionRequest};
use iii::function::FunctionResult;
use iii::observer::{ObserveRequest, ObserversConfig};
use iii::protocol::{ObserveSource, ObserveTarget, ObserverState};
use iii::worker_connections::WorkerConnection;
use iii::workers::observability::metrics::ensure_default_meter;
use tokio::sync::mpsc;
use uuid::Uuid;

const SINK: &str = "bench::sink";

/// Observers measured at each step.
const WINDOW: usize = 512;

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
    format!("bench::fn-{}", if distinct { i } else { 0 })
}

fn request(i: usize, distinct: bool) -> ObserveRequest {
    ObserveRequest {
        id: format!("obs-{i}"),
        source: ObserveSource {
            function_id: function_of(i, distinct),
            namespace: None,
            args: None,
            filters: None,
            capture: None,
            on: None,
            deliver: None,
            raw_input: true,
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

/// A worker connection whose socket is drained.
fn worker(engine: &Engine) -> WorkerConnection {
    let (tx, mut rx) = mpsc::channel::<Outbound>(1 << 16);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let worker = WorkerConnection::new(tx);
    engine.worker_registry.register_worker(worker.clone());
    worker
}

async fn engine_with(functions: usize) -> Arc<Engine> {
    let engine = Arc::new(Engine::new());
    engine.observers.set_config(ObserversConfig {
        max_per_connection: 1 << 20,
        max_per_function: 1 << 20,
        ..ObserversConfig::default()
    });
    register(&engine, SINK);
    for i in 0..functions {
        register(&engine, &function_of(i, true));
    }
    engine
}

async fn active(engine: &Engine, i: usize) {
    let id = format!("obs-{i}");
    let deadline = Instant::now() + Duration::from_secs(60);
    while engine.observers.get(&id).map(|o| o.state()) != Some(ObserverState::Active) {
        assert!(Instant::now() < deadline, "`{id}` never became active");
        tokio::task::yield_now().await;
    }
}

/// Creates observers `from..to` and waits until the last is active.
async fn create(
    engine: &Arc<Engine>,
    worker: &WorkerConnection,
    from: usize,
    to: usize,
    distinct: bool,
) {
    for i in from..to {
        engine
            .observers
            .observe(engine, worker, request(i, distinct))
            .await;
    }
    active(engine, to - 1).await;
}

fn median(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    samples[samples.len() / 2]
}

/// µs per `observe` once `n` observers exist.
async fn observe_costs(steps: &[usize], distinct: bool) -> Vec<f64> {
    let max = *steps.last().unwrap();
    let engine = engine_with(if distinct { max } else { 1 }).await;
    let worker = worker(&engine);
    let mut created = 0;
    let mut costs = Vec::new();
    for &n in steps {
        create(&engine, &worker, created, n - WINDOW, distinct).await;
        let start = Instant::now();
        create(&engine, &worker, n - WINDOW, n, distinct).await;
        costs.push(start.elapsed().as_secs_f64() * 1e6 / WINDOW as f64);
        created = n;
    }
    costs
}

/// ms that `connection_closed` takes for a connection with `n` observers.
async fn disconnect_cost(n: usize, distinct: bool) -> f64 {
    let engine = engine_with(if distinct { n } else { 1 }).await;
    let worker = worker(&engine);
    create(&engine, &worker, 0, n, distinct).await;
    let start = Instant::now();
    engine
        .observers
        .connection_closed(worker.id, Uuid::new_v4());
    start.elapsed().as_secs_f64() * 1e3
}

fn main() {
    ensure_default_meter();
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(async {
        let steps = [1024, 2048, 4096, 8192, 16_384, 32_768];
        println!("observe: µs per observe once n observers exist (median of 3)");
        println!("{:>8} {:>12} {:>14}", "n", "distinct", "one function");
        let mut distinct = Vec::new();
        let mut same = Vec::new();
        for _ in 0..3 {
            distinct.push(observe_costs(&steps, true).await);
            same.push(observe_costs(&steps, false).await);
        }
        for (i, n) in steps.iter().enumerate() {
            println!(
                "{n:>8} {:>12.1} {:>14.1}",
                median(distinct.iter().map(|run| run[i]).collect()),
                median(same.iter().map(|run| run[i]).collect()),
            );
        }

        println!();
        println!("disconnect: ms for connection_closed with n observers (median of 3)");
        println!("{:>8} {:>12} {:>14}", "n", "distinct", "one function");
        for n in [1024, 2048, 4096, 8192, 16_384] {
            let mut d = Vec::new();
            let mut s = Vec::new();
            for _ in 0..3 {
                d.push(disconnect_cost(n, true).await);
                s.push(disconnect_cost(n, false).await);
            }
            println!("{n:>8} {:>12.2} {:>14.2}", median(d), median(s));
        }
    });
}
