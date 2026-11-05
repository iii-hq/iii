//! Memory of the window of routed calls an `observe.v2` host answered, kept
//! by its connection (`iii::observer::RoutedAnswers`, decision 38), measured
//! with a counting global allocator (it applies to this test binary only).
//!
//! Every test reads process-wide counters: run them with
//! `--test-threads=1 --nocapture`. Each figure is the median of 3
//! repetitions, read once nothing is in flight.

mod observe_support;

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::{
        Arc,
        atomic::{AtomicIsize, Ordering},
    },
    time::{Duration, Instant},
};

use iii::observer::{ObserversConfig, RoutedAnswers};
use observe_support::{Client, OBSERVE_V2, connect, connect_with, registered, spawn_engine};
use serde_json::json;
use uuid::Uuid;

struct Counting;

/// Bytes allocated and not freed.
static LIVE: AtomicIsize = AtomicIsize::new(0);
/// Blocks allocated and not freed.
static BLOCKS: AtomicIsize = AtomicIsize::new(0);
/// Allocations made, freed or not (a reallocation counts as one).
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

#[derive(Clone, Copy, Debug)]
struct Counters {
    live: isize,
    blocks: isize,
    allocs: isize,
}

fn counters() -> Counters {
    Counters {
        live: LIVE.load(Ordering::Relaxed),
        blocks: BLOCKS.load(Ordering::Relaxed),
        allocs: ALLOCS.load(Ordering::Relaxed),
    }
}

impl std::ops::Sub for Counters {
    type Output = Counters;
    fn sub(self, before: Counters) -> Counters {
        Counters {
            live: self.live - before.live,
            blocks: self.blocks - before.blocks,
            allocs: self.allocs - before.allocs,
        }
    }
}

fn median<T: Copy + Ord>(mut values: Vec<T>) -> T {
    values.sort_unstable();
    values[values.len() / 2]
}

/// The medians of `reps`, field by field.
fn medians(reps: &[Counters]) -> Counters {
    Counters {
        live: median(reps.iter().map(|c| c.live).collect()),
        blocks: median(reps.iter().map(|c| c.blocks).collect()),
        allocs: median(reps.iter().map(|c| c.allocs).collect()),
    }
}

/// What one connection's window holds once its host answered `answers`
/// distinct routed calls (live bytes and blocks, the `Arc` the connection
/// keeps it in included) and the allocations recording them made.
fn window_after(answers: usize) -> Counters {
    let ids: Vec<Uuid> = (0..answers).map(|_| Uuid::new_v4()).collect();
    let before = counters();
    let answered = Arc::new(RoutedAnswers::default());
    for id in &ids {
        answered.record(*id);
    }
    let held = counters() - before;
    drop(answered);
    held
}

#[test]
fn window_bytes_per_connection() {
    println!("routed-answers window, per connection (median of 3)");
    println!("answers | live bytes | live blocks | allocations made");
    for answers in [0, 100, 1_000, 4_096, 5_000, 50_000] {
        let reps: Vec<Counters> = (0..3).map(|_| window_after(answers)).collect();
        let held = medians(&reps);
        println!(
            "{answers} | {} | {} | {}",
            held.live, held.blocks, held.allocs
        );
    }
}

/// Nanoseconds per `record` from a plain loop, in a debug build: indicative
/// only, to compare the base and the change on the same machine.
#[test]
fn window_ns_per_record() {
    const KEPT: usize = 4_096;
    const N: usize = 200_000;
    let ids: Vec<Uuid> = (0..KEPT + N).map(|_| Uuid::new_v4()).collect();
    let per = |elapsed: Duration, records: usize| elapsed.as_nanos() / records as u128;
    let (mut filling, mut evicting, mut repeated) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..3 {
        let answered = RoutedAnswers::default();
        let start = Instant::now();
        for id in &ids[..KEPT] {
            answered.record(*id);
        }
        filling.push(per(start.elapsed(), KEPT));
        // The window is full: each new id forgets the oldest.
        let start = Instant::now();
        for id in &ids[KEPT..] {
            answered.record(*id);
        }
        evicting.push(per(start.elapsed(), N));
        // An id the window keeps already.
        let kept = ids[KEPT + N - 1];
        let start = Instant::now();
        for _ in 0..N {
            answered.record(kept);
        }
        repeated.push(per(start.elapsed(), N));
    }
    println!("routed-answers record, ns per call (debug build, indicative only, median of 3)");
    println!("filling (first {KEPT}) | {}", median(filling));
    println!("full, evicting the oldest | {}", median(evicting));
    println!("an id already kept | {}", median(repeated));
}

/// `calls` routed calls of `orders::create` from `checkout`, each answered
/// by `host`, one after another.
async fn answer(checkout: &mut Client, host: &mut Client, calls: usize) {
    for n in 0..calls {
        let call = checkout.call("orders::create", json!({ "n": n })).await;
        let invoke = host.invoked("orders::create").await;
        host.reply(&invoke, json!({ "id": n })).await;
        checkout.result(call).await;
    }
}

/// Lets the engine finish what answering started before the counters are
/// read.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(300)).await;
}

/// What the engine holds more once an `observe.v2` host answered 5,000
/// routed calls than once it answered 100, over real WebSockets: its
/// connection's window, and anything else answering leaves behind. Nothing
/// observes the function: every answer of an `observe.v2` host is kept.
#[tokio::test]
async fn engine_bytes_per_connection() {
    let mut growth = Vec::new();
    for _ in 0..3 {
        let t = spawn_engine(ObserversConfig::default()).await;
        let mut host = connect_with(&t, "orders", OBSERVE_V2).await;
        host.register_function("orders::create").await;
        registered(&t.engine, "default", "orders::create").await;
        let mut checkout = connect(&t, "checkout", true).await;
        answer(&mut checkout, &mut host, 100).await;
        settle().await;
        let after_100 = counters();
        answer(&mut checkout, &mut host, 4_900).await;
        settle().await;
        growth.push(counters() - after_100);
    }
    let held = medians(&growth);
    println!("engine, one observe.v2 host, from 100 to 5,000 answers (median of 3)");
    println!("live bytes | live blocks | allocations made");
    println!("{} | {} | {}", held.live, held.blocks, held.allocs);
}
