//! `release_freed_memory` must hand fragmented, freed heap back to the OS
//! (MOT-4733). Lives in its own test binary so no other test's allocations
//! move the RSS readings.
#![cfg(all(target_os = "linux", target_env = "gnu"))]

use iii::memory::{release_freed_memory, resident_bytes};

const MIB: u64 = 1 << 20;

#[test]
fn trim_returns_fragmented_freed_heap_to_the_os() {
    // 16 KiB chunks sit below glibc's mmap threshold, so they live inside the
    // arenas like span payloads do. Freeing every other one leaves the heap
    // fragmented the way a span storm does: no top chunk to trim, and glibc
    // keeps every page resident until something calls malloc_trim.
    const CHUNK: usize = 16 * 1024;
    const COUNT: usize = 32 * 1024; // 512 MiB live at the peak

    let baseline = resident_bytes().expect("statm readable");
    let mut chunks: Vec<Option<Vec<u8>>> = (0..COUNT)
        .map(|i| Some(vec![(i & 0xff) as u8; CHUNK]))
        .collect();
    let peak = resident_bytes().expect("statm readable");
    assert!(
        peak - baseline >= 400 * MIB,
        "allocation should be resident: baseline {baseline} peak {peak}"
    );

    for slot in chunks.iter_mut().step_by(2) {
        *slot = None;
    }
    let after_free = resident_bytes().expect("statm readable");

    let (before, after) = release_freed_memory().expect("glibc target");
    assert!(
        after_free.saturating_sub(after) >= 128 * MIB,
        "trim should release most of the 256 MiB freed: after_free {after_free} before {before} after {after}"
    );

    // The live half is intact.
    assert!(chunks.iter().flatten().all(|c| c.len() == CHUNK));
}
