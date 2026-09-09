//! Give freed heap back to the operating system.
//!
//! glibc malloc keeps freed chunks inside its per-thread arenas and only
//! returns the top of the main arena on its own. After a burst of small
//! allocations, such as a span storm churning through the bounded hot span
//! cache, the process keeps its peak RSS long after the live data is back
//! under its cap: one Linkly run left the engine at 14 GB with 236 MB of
//! spans in memory (MOT-4733). `malloc_trim(0)` walks every arena and
//! `madvise`s the freed pages away, so the observability sweep calls
//! [`release_freed_memory`] once a minute.

/// Resident set size of this process in bytes, read from `/proc/self/statm`.
/// `None` where that file does not exist.
pub fn resident_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * page_size())
}

#[cfg(unix)]
fn page_size() -> u64 {
    // SAFETY: sysconf has no preconditions and only reads a constant.
    let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if size > 0 { size as u64 } else { 4096 }
}

#[cfg(not(unix))]
fn page_size() -> u64 {
    4096
}

/// Return freed heap pages to the OS. Returns the RSS before and after the
/// trim, in bytes, on Linux with glibc; `None` where the allocator already
/// releases memory on its own (musl, macOS, Windows) or RSS cannot be read.
///
/// Cheap on a small heap. On a heap that just churned through gigabytes it
/// takes an arena lock at a time while it `madvise`s, so call it off the
/// async executor.
pub fn release_freed_memory() -> Option<(u64, u64)> {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        let before = resident_bytes()?;
        // SAFETY: malloc_trim only touches allocator bookkeeping; it is safe
        // to call from any thread at any time.
        unsafe {
            libc::malloc_trim(0);
        }
        let after = resident_bytes()?;
        Some((before, after))
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    {
        None
    }
}
