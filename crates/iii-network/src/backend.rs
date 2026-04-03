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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::SharedState;
    use msb_krun::backends::net::NetBackend;

    fn make_backend(capacity: usize) -> SmoltcpBackend {
        let shared = Arc::new(SharedState::new(capacity));
        SmoltcpBackend::new(shared)
    }

    #[test]
    fn write_frame_strips_header_and_enqueues() {
        let mut backend = make_backend(16);
        let hdr_len = VIRTIO_NET_HDR_LEN;
        let mut buf = vec![0u8; hdr_len + 4];
        buf[hdr_len..].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]);

        backend.write_frame(hdr_len, &mut buf).unwrap();

        let frame = backend.shared.tx_ring.pop().unwrap();
        assert_eq!(frame, vec![0xAA, 0xBB, 0xCC, 0xDD]);
    }

    #[test]
    fn write_frame_tracks_tx_bytes() {
        let mut backend = make_backend(16);
        let hdr_len = VIRTIO_NET_HDR_LEN;
        let mut buf = vec![0u8; hdr_len + 10];

        backend.write_frame(hdr_len, &mut buf).unwrap();
        assert_eq!(backend.shared.tx_bytes(), 10);
    }

    #[test]
    fn write_frame_returns_error_when_queue_full() {
        let mut backend = make_backend(1);
        let hdr_len = VIRTIO_NET_HDR_LEN;
        let mut buf = vec![0u8; hdr_len + 4];

        backend.write_frame(hdr_len, &mut buf).unwrap();
        let result = backend.write_frame(hdr_len, &mut buf);
        assert!(result.is_err());
    }

    #[test]
    fn read_frame_prepends_zeroed_header() {
        let mut backend = make_backend(16);
        let frame_data = vec![0xDE, 0xAD, 0xBE, 0xEF];
        backend.shared.rx_ring.push(frame_data).unwrap();

        let mut buf = vec![0xFFu8; 64];
        let len = backend.read_frame(&mut buf).unwrap();

        assert_eq!(len, VIRTIO_NET_HDR_LEN + 4);
        // Header should be zeroed
        assert!(buf[..VIRTIO_NET_HDR_LEN].iter().all(|&b| b == 0));
        // Payload should follow
        assert_eq!(
            &buf[VIRTIO_NET_HDR_LEN..VIRTIO_NET_HDR_LEN + 4],
            &[0xDE, 0xAD, 0xBE, 0xEF]
        );
    }

    #[test]
    fn read_frame_returns_error_when_empty() {
        let mut backend = make_backend(16);
        let mut buf = vec![0u8; 64];
        let result = backend.read_frame(&mut buf);
        assert!(result.is_err());
    }

    #[test]
    fn read_frame_drops_oversized_frame() {
        let mut backend = make_backend(16);
        let frame_data = vec![0u8; 100];
        backend.shared.rx_ring.push(frame_data).unwrap();

        // Buffer too small for header + frame
        let mut buf = vec![0u8; 20];
        let result = backend.read_frame(&mut buf);
        assert!(result.is_err());
    }

    #[test]
    fn has_unfinished_write_always_false() {
        let backend = make_backend(16);
        assert!(!backend.has_unfinished_write());
    }

    #[test]
    fn try_finish_write_always_ok() {
        let mut backend = make_backend(16);
        assert!(backend.try_finish_write(0, &[]).is_ok());
    }

    #[test]
    fn raw_socket_fd_returns_valid_fd() {
        let backend = make_backend(16);
        let fd = backend.raw_socket_fd();
        assert!(fd >= 0);
    }
}
