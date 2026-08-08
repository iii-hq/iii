//! Shared state between the NetWorker thread, smoltcp poll thread, and tokio
//! proxy tasks.
//!
//! All inter-thread communication flows through [`SharedState`], which holds
//! lock-free frame queues and cross-platform [`WakePipe`] notifications.

use crossbeam_queue::ArrayQueue;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use crate::wake_pipe::WakePipe;

/// Default frame queue capacity. Matches libkrun's virtio queue size.
pub const DEFAULT_QUEUE_CAPACITY: usize = 1024;

/// Network-activity beacon: a file whose mtime is refreshed whenever the
/// stack relays guest **payload** (TCP proxy data, UDP datagrams, DNS
/// queries). The sandbox daemon's idle reaper stats it — this file IS the
/// daemon<->vm-boot activity channel, chosen because both processes already
/// share the sandbox dir and a crashed writer needs no cleanup protocol.
///
/// Payload-level on purpose, not frame-level: the frame counters
/// ([`SharedState::add_tx_bytes`]) also tick for ARP, pure ACKs, and TCP
/// keepalive probes. Many client runtimes enable TCP keepalive by default
/// (Go's net/http probes every 15s), so a frame-level beacon would keep an
/// idle-but-connected guest alive forever and the idle timeout would never
/// fire. A sandbox only counts as active when data actually moves.
///
/// Touches are throttled to once per second — the reaper's granularity is
/// tens of seconds, and per-packet `futimens` would be pure overhead.
pub struct ActivityStamp {
    file: std::fs::File,
    /// Unix-seconds of the last touch; CAS gate so concurrent relay tasks
    /// collapse to ~one syscall per second.
    last_touch_secs: AtomicU64,
}

impl ActivityStamp {
    /// Create (or reuse) the beacon file and stamp "active now", so a
    /// sandbox that boots with networking but never sends a byte reads as
    /// idle-since-boot, exactly like `last_exec_at` at create time.
    pub fn create(path: &Path) -> std::io::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(path)?;
        file.set_modified(SystemTime::now())?;
        Ok(Self {
            file,
            last_touch_secs: AtomicU64::new(0),
        })
    }

    fn touch(&self) {
        let now_secs = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let prev = self.last_touch_secs.load(Ordering::Relaxed);
        if prev == now_secs {
            return; // already touched this second
        }
        // Losers of the race skip; an occasional extra touch (clock step,
        // second rollover) is harmless — the beacon only needs freshness.
        if self
            .last_touch_secs
            .compare_exchange(prev, now_secs, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            let _ = self.file.set_modified(SystemTime::now());
        }
    }
}

/// All shared state between the three threads:
///
/// - **NetWorker** (libkrun) — pushes guest frames to `tx_ring`, pops
///   response frames from `rx_ring`.
/// - **smoltcp poll thread** — pops from `tx_ring`, processes through smoltcp,
///   pushes responses to `rx_ring`.
/// - **tokio proxy tasks** — relay data between smoltcp sockets and real
///   network connections.
///
/// Queue naming follows the **guest's perspective** (matching libkrun's
/// convention): `tx_ring` = "transmit from guest", `rx_ring` = "receive at
/// guest".
pub struct SharedState {
    /// Frames from guest → smoltcp (NetWorker writes, smoltcp reads).
    pub tx_ring: ArrayQueue<Vec<u8>>,

    /// Frames from smoltcp → guest (smoltcp writes, NetWorker reads).
    pub rx_ring: ArrayQueue<Vec<u8>>,

    /// Wakes NetWorker: "rx_ring has frames for the guest."
    pub rx_wake: WakePipe,

    /// Wakes smoltcp poll thread: "tx_ring has frames from the guest."
    pub tx_wake: WakePipe,

    /// Wakes smoltcp poll thread: "proxy task has data to write to a smoltcp
    /// socket."
    pub proxy_wake: WakePipe,

    metrics: NetworkMetrics,

    /// Idle-reaper beacon; `None` when the spawner didn't ask for one
    /// (workers outside the sandbox daemon, tests).
    activity: Option<ActivityStamp>,
}

struct NetworkMetrics {
    tx_bytes: AtomicU64,
    rx_bytes: AtomicU64,
}

impl SharedState {
    /// Create shared state with the given queue capacity.
    pub fn new(queue_capacity: usize) -> Self {
        Self::with_activity(queue_capacity, None)
    }

    /// Create shared state carrying an [`ActivityStamp`] beacon.
    pub fn with_activity(queue_capacity: usize, activity: Option<ActivityStamp>) -> Self {
        Self {
            tx_ring: ArrayQueue::new(queue_capacity),
            rx_ring: ArrayQueue::new(queue_capacity),
            rx_wake: WakePipe::new(),
            tx_wake: WakePipe::new(),
            proxy_wake: WakePipe::new(),
            metrics: NetworkMetrics::default(),
            activity,
        }
    }

    /// Record "guest payload moved just now" on the beacon, if one exists.
    /// Called by the TCP proxy, UDP relay, and DNS resolver on actual data —
    /// never on bare frames. See [`ActivityStamp`] for why.
    pub fn note_activity(&self) {
        if let Some(a) = &self.activity {
            a.touch();
        }
    }

    /// Increment the guest -> runtime byte counter.
    pub fn add_tx_bytes(&self, bytes: usize) {
        self.metrics
            .tx_bytes
            .fetch_add(bytes as u64, Ordering::Relaxed);
    }

    /// Increment the runtime -> guest byte counter.
    pub fn add_rx_bytes(&self, bytes: usize) {
        self.metrics
            .rx_bytes
            .fetch_add(bytes as u64, Ordering::Relaxed);
    }

    /// Total bytes transmitted by the guest into the runtime.
    pub fn tx_bytes(&self) -> u64 {
        self.metrics.tx_bytes.load(Ordering::Relaxed)
    }

    /// Total bytes delivered by the runtime to the guest.
    pub fn rx_bytes(&self) -> u64 {
        self.metrics.rx_bytes.load(Ordering::Relaxed)
    }
}

impl Default for NetworkMetrics {
    fn default() -> Self {
        Self {
            tx_bytes: AtomicU64::new(0),
            rx_bytes: AtomicU64::new(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_state_queue_push_pop() {
        let state = SharedState::new(4);

        state.tx_ring.push(vec![1, 2, 3]).unwrap();
        state.tx_ring.push(vec![4, 5, 6]).unwrap();

        assert_eq!(state.tx_ring.pop(), Some(vec![1, 2, 3]));
        assert_eq!(state.tx_ring.pop(), Some(vec![4, 5, 6]));
        assert_eq!(state.tx_ring.pop(), None);
    }

    #[test]
    fn shared_state_queue_full() {
        let state = SharedState::new(2);

        state.rx_ring.push(vec![1]).unwrap();
        state.rx_ring.push(vec![2]).unwrap();
        assert!(state.rx_ring.push(vec![3]).is_err());
    }

    /// Without a beacon, note_activity must be a no-op — every non-sandbox
    /// caller of this stack goes through this path on hot relay loops.
    #[test]
    fn note_activity_without_beacon_is_a_noop() {
        let state = SharedState::new(2);
        state.note_activity();
    }

    fn temp_beacon(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "iii-net-activity-test-{}-{name}",
            std::process::id()
        ))
    }

    /// The regression this beacon exists for: relayed payload must move the
    /// file's mtime forward so the reaper sees the sandbox as active.
    #[test]
    fn note_activity_refreshes_the_beacon_mtime() {
        let path = temp_beacon("refresh");
        let state = SharedState::with_activity(2, Some(ActivityStamp::create(&path).unwrap()));

        // Age the beacon far into the past through a second handle, then
        // stamp. No sleeps: "moved forward" is asserted against the aged
        // mtime, not against wall-clock deltas.
        let old = SystemTime::now() - std::time::Duration::from_secs(600);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(old)
            .unwrap();

        state.note_activity();

        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert!(
            mtime > old + std::time::Duration::from_secs(300),
            "note_activity must refresh the beacon mtime, got {mtime:?} vs aged {old:?}"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// Touches are throttled to once per second: a second stamp within the
    /// same second must NOT hit the filesystem again.
    #[test]
    fn note_activity_throttles_within_one_second() {
        let path = temp_beacon("throttle");
        // Retry on the (rare) run that straddles a second boundary between
        // the two stamps — the throttle gate is per-unix-second.
        for attempt in 0..3 {
            let state = SharedState::with_activity(2, Some(ActivityStamp::create(&path).unwrap()));
            let second_of =
                |t: SystemTime| t.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();

            let before = SystemTime::now();
            state.note_activity(); // consumes this second's touch

            let old = SystemTime::now() - std::time::Duration::from_secs(600);
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(old)
                .unwrap();

            state.note_activity(); // same second: must be throttled
            let after = SystemTime::now();

            if second_of(before) != second_of(after) {
                continue; // straddled a boundary; not a valid trial
            }
            let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
            assert!(
                mtime < old + std::time::Duration::from_secs(1),
                "second stamp in the same second must be throttled (attempt {attempt})"
            );
            break;
        }
        let _ = std::fs::remove_file(&path);
    }
}
