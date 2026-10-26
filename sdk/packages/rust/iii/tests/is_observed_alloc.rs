//! `FunctionRef::is_observed()` is the emit fast path: one atomic load and
//! no allocation. A test binary of its own, with a counting allocator.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

use iii_sdk::{Error, IIIClient, RegisterFunction};
use serde_json::Value;

struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
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

#[test]
fn is_observed_does_not_allocate() {
    let iii = IIIClient::new("ws://127.0.0.1:1");
    let function = iii.register_function(
        "alloc::f",
        RegisterFunction::new(|input: Value| Ok::<_, Error>(input)),
    );
    let before = ALLOCATIONS.load(Ordering::SeqCst);
    let mut observed = 0;
    for _ in 0..10_000 {
        if std::hint::black_box(&function).is_observed() {
            observed += 1;
        }
    }
    let after = ALLOCATIONS.load(Ordering::SeqCst);
    assert_eq!(observed, 0);
    assert_eq!(after - before, 0, "is_observed() allocated");

    // Positive control: the counter does see an allocation.
    let before = ALLOCATIONS.load(Ordering::SeqCst);
    let boxed = std::hint::black_box(Box::new(42u64));
    assert!(
        ALLOCATIONS.load(Ordering::SeqCst) > before,
        "the counter is live"
    );
    drop(boxed);
}
