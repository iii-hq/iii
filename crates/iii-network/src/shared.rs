//! Shared state between the NetWorker thread, smoltcp poll thread, and tokio
//! proxy tasks.
//!
//! All inter-thread communication flows through [`SharedState`], which holds
//! lock-free frame queues and cross-platform [`WakePipe`] notifications.

use crossbeam_queue::ArrayQueue;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::wake_pipe::WakePipe;

/// Default frame queue capacity. Matches libkrun's virtio queue size.
pub const DEFAULT_QUEUE_CAPACITY: usize = 1024;

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
}

struct NetworkMetrics {
    tx_bytes: AtomicU64,
    rx_bytes: AtomicU64,
}

impl SharedState {
    /// Create shared state with the given queue capacity.
    pub fn new(queue_capacity: usize) -> Self {
        Self {
            tx_ring: ArrayQueue::new(queue_capacity),
            rx_ring: ArrayQueue::new(queue_capacity),
            rx_wake: WakePipe::new(),
            tx_wake: WakePipe::new(),
            proxy_wake: WakePipe::new(),
            metrics: NetworkMetrics::default(),
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
}
