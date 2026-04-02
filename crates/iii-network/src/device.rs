//! Slot-based [`smoltcp::phy::Device`] implementation.
//!
//! [`SmoltcpDevice`] bridges [`SharedState`]'s lock-free queues to smoltcp's
//! token-based `Device` API. It uses a **single-frame slot** design: the poll
//! loop pops a frame from `tx_ring` via [`stage_next_frame()`], inspects it
//! (creating TCP sockets before smoltcp sees a SYN), then smoltcp consumes
//! the staged frame via [`receive()`](smoltcp::phy::Device::receive).
//!
//! [`stage_next_frame()`]: SmoltcpDevice::stage_next_frame

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use smoltcp::phy::{self, DeviceCapabilities, Medium};
use smoltcp::time::Instant;

use crate::shared::SharedState;

/// smoltcp device backed by [`SharedState`]'s lock-free queues.
///
/// # Slot-based design
///
/// The poll loop controls when frames are popped from `tx_ring`:
///
/// 1. Call [`stage_next_frame()`](Self::stage_next_frame) to pop a frame and
///    inspect it.
/// 2. Optionally call [`drop_staged_frame()`](Self::drop_staged_frame) to
///    discard the frame (e.g. non-DNS UDP handled outside smoltcp).
/// 3. When smoltcp's `iface.poll()` calls `receive()`, the staged frame is
///    consumed.
pub struct SmoltcpDevice {
    shared: Arc<SharedState>,
    mtu: usize,
    /// Single-frame slot. Set by the poll loop via `stage_next_frame()`,
    /// consumed by smoltcp's `poll()` via `receive()`.
    pending_rx: Option<Vec<u8>>,
    /// Set by `TxToken::consume` when a frame is pushed to `rx_ring`.
    /// The poll loop checks this flag after the egress loop and calls
    /// `rx_wake.wake()` once instead of per-frame (coalesced wakes).
    pub(crate) frames_emitted: AtomicBool,
}

/// Token returned by the `Device::receive()` implementation — delivers one
/// frame from the guest to smoltcp.
pub struct SmoltcpRxToken {
    frame: Vec<u8>,
}

/// Token returned by the `Device::receive()` and `Device::transmit()`
/// implementations — sends one frame from smoltcp to the guest.
pub struct SmoltcpTxToken<'a> {
    device: &'a mut SmoltcpDevice,
}

impl SmoltcpDevice {
    /// Create a new device connected to the given shared state.
    ///
    /// `mtu` is the IP-level MTU (e.g. 1500). The Ethernet frame MTU reported
    /// to smoltcp is `mtu + 14` (Ethernet header).
    pub fn new(shared: Arc<SharedState>, mtu: usize) -> Self {
        Self {
            shared,
            mtu,
            pending_rx: None,
            frames_emitted: AtomicBool::new(false),
        }
    }

    /// Pop the next frame from `tx_ring` into the slot for inspection.
    ///
    /// Called by the poll loop **before** `iface.poll()`. Returns a reference
    /// to the staged frame, or `None` if the queue is empty. Repeated calls
    /// return the same frame until it is consumed or dropped.
    pub fn stage_next_frame(&mut self) -> Option<&[u8]> {
        if self.pending_rx.is_none() {
            self.pending_rx = self.shared.tx_ring.pop();
        }
        self.pending_rx.as_deref()
    }

    /// Discard the staged frame without letting smoltcp process it.
    ///
    /// Used for frames handled outside smoltcp (e.g. non-DNS UDP relay).
    pub fn drop_staged_frame(&mut self) {
        self.pending_rx = None;
    }
}

impl phy::Device for SmoltcpDevice {
    type RxToken<'a> = SmoltcpRxToken;
    type TxToken<'a> = SmoltcpTxToken<'a>;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let frame = self.pending_rx.take()?;
        Some((SmoltcpRxToken { frame }, SmoltcpTxToken { device: self }))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        if self.shared.rx_ring.len() < self.shared.rx_ring.capacity() {
            Some(SmoltcpTxToken { device: self })
        } else {
            None
        }
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ethernet;
        caps.max_transmission_unit = self.mtu + 14;
        caps
    }
}

impl phy::RxToken for SmoltcpRxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(&self.frame)
    }
}

impl<'a> phy::TxToken for SmoltcpTxToken<'a> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buf = vec![0u8; len];
        let result = f(&mut buf);
        self.device.shared.add_rx_bytes(buf.len());
        let _ = self.device.shared.rx_ring.push(buf);
        self.device.frames_emitted.store(true, Ordering::Relaxed);
        result
    }
}
