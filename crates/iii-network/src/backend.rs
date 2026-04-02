//! `SmoltcpBackend` — libkrun [`NetBackend`] implementation that bridges the
//! NetWorker thread to the smoltcp poll thread via lock-free queues.
//!
//! The NetWorker calls [`write_frame()`](NetBackend::write_frame) when the
//! guest sends a frame and [`read_frame()`](NetBackend::read_frame) to deliver
//! frames back to the guest. Frames flow through [`SharedState`]'s
//! `tx_ring`/`rx_ring` queues with [`WakePipe`](crate::wake_pipe::WakePipe)
//! notifications.

use std::{os::fd::RawFd, sync::Arc};

use msb_krun::backends::net::{NetBackend, ReadError, WriteError};

use crate::shared::SharedState;

/// Size of the virtio-net header (`virtio_net_hdr_v1`): 12 bytes.
///
/// libkrun's NetWorker prepends this header to every frame buffer. The
/// backend must strip it on TX (guest → smoltcp) and prepend a zeroed
/// header on RX (smoltcp → guest).
const VIRTIO_NET_HDR_LEN: usize = 12;

/// Network backend that bridges libkrun's NetWorker to smoltcp via lock-free
/// queues.
///
/// - **TX path** (`write_frame`): strips the virtio-net header, pushes the
///   ethernet frame to `tx_ring`, wakes the smoltcp poll thread.
/// - **RX path** (`read_frame`): pops a frame from `rx_ring`, prepends a
///   zeroed virtio-net header for the guest.
/// - **Wake fd** (`raw_socket_fd`): returns `rx_wake`'s read end so the
///   NetWorker's epoll can detect new frames.
pub struct SmoltcpBackend {
    shared: Arc<SharedState>,
}

impl SmoltcpBackend {
    /// Create a new backend connected to the given shared state.
    pub fn new(shared: Arc<SharedState>) -> Self {
        Self { shared }
    }
}

impl NetBackend for SmoltcpBackend {
    /// Guest is sending a frame. Strip the virtio-net header and enqueue
    /// the raw ethernet frame for smoltcp.
    fn write_frame(&mut self, hdr_len: usize, buf: &mut [u8]) -> Result<(), WriteError> {
        let ethernet_frame = buf[hdr_len..].to_vec();
        self.shared.add_tx_bytes(ethernet_frame.len());
        self.shared
            .tx_ring
            .push(ethernet_frame)
            .map_err(|_| WriteError::NothingWritten)?;
        self.shared.tx_wake.wake();
        Ok(())
    }

    /// Deliver a frame from smoltcp to the guest. Prepends a zeroed
    /// virtio-net header.
    fn read_frame(&mut self, buf: &mut [u8]) -> Result<usize, ReadError> {
        let frame = self.shared.rx_ring.pop().ok_or(ReadError::NothingRead)?;

        let total_len = VIRTIO_NET_HDR_LEN + frame.len();
        if total_len > buf.len() {
            tracing::debug!(
                frame_len = frame.len(),
                buf_len = buf.len(),
                "dropping oversized frame from rx_ring"
            );
            return Err(ReadError::NothingRead);
        }

        buf[..VIRTIO_NET_HDR_LEN].fill(0);
        buf[VIRTIO_NET_HDR_LEN..total_len].copy_from_slice(&frame);

        Ok(total_len)
    }

    /// No partial writes — queue push is atomic.
    fn has_unfinished_write(&self) -> bool {
        false
    }

    /// No partial writes — nothing to finish.
    fn try_finish_write(&mut self, _hdr_len: usize, _buf: &[u8]) -> Result<(), WriteError> {
        Ok(())
    }

    /// File descriptor for NetWorker's epoll. Becomes readable when
    /// `rx_ring` has frames for the guest.
    fn raw_socket_fd(&self) -> RawFd {
        self.shared.rx_wake.as_raw_fd()
    }
}
