//! Integration tests for iii-network.
//!
//! These tests verify that the network subsystem components work together
//! correctly: SharedState queues, SmoltcpBackend frame bridging, SmoltcpDevice
//! frame staging, and ConnectionTracker lifecycle management.

use std::sync::Arc;

use iii_network::{ConnectionTracker, SharedState, SmoltcpBackend, SmoltcpDevice};
use msb_krun::backends::net::NetBackend;
use smoltcp::iface::SocketSet;
use smoltcp::phy::Device;
use smoltcp::time::Instant;

/// Test 3: Network connectivity — frame flow from backend through shared state to device.
///
/// Verifies the TX path: SmoltcpBackend::write_frame -> SharedState::tx_ring -> SmoltcpDevice::stage_next_frame.
#[test]
fn backend_write_frame_flows_to_device_stage() {
    let shared = Arc::new(SharedState::new(64));

    // Backend writes a frame (stripping the virtio-net header)
    let mut backend = SmoltcpBackend::new(shared.clone());
    let hdr_len = 12; // VIRTIO_NET_HDR_LEN
    let mut frame_with_header = vec![0u8; hdr_len + 6];
    frame_with_header[hdr_len..].copy_from_slice(&[0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
    backend
        .write_frame(hdr_len, &mut frame_with_header)
        .unwrap();

    // Device reads the frame from tx_ring
    let mut device = SmoltcpDevice::new(shared, 1500);
    let staged = device.stage_next_frame();
    assert!(staged.is_some());
    assert_eq!(staged.unwrap(), &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
}

/// Test 3 (continued): RX path — device transmit token pushes frames to rx_ring,
/// backend reads them with prepended virtio-net header.
#[test]
fn device_transmit_flows_to_backend_read() {
    let shared = Arc::new(SharedState::new(64));

    // Device transmit: push a frame to rx_ring
    let mut device = SmoltcpDevice::new(shared.clone(), 1500);
    let tx_token = device.transmit(Instant::from_millis(0)).unwrap();
    smoltcp::phy::TxToken::consume(tx_token, 4, |buf| {
        buf.copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
    });

    // Backend reads the frame with prepended virtio-net header
    let mut backend = SmoltcpBackend::new(shared);
    let mut buf = vec![0xFFu8; 64];
    let len = backend.read_frame(&mut buf).unwrap();
    assert_eq!(len, 12 + 4); // header + payload
    assert!(buf[..12].iter().all(|&b| b == 0)); // zeroed header
    assert_eq!(&buf[12..16], &[0xDE, 0xAD, 0xBE, 0xEF]);
}

/// Test 3 (continued): Shared state metrics track bytes accurately across
/// backend write and device transmit operations.
#[test]
fn shared_state_metrics_track_bidirectional_bytes() {
    let shared = Arc::new(SharedState::new(64));

    // TX path: backend writes 10 bytes of payload
    let mut backend = SmoltcpBackend::new(shared.clone());
    let hdr_len = 12;
    let mut buf = vec![0u8; hdr_len + 10];
    backend.write_frame(hdr_len, &mut buf).unwrap();

    assert_eq!(shared.tx_bytes(), 10);
    assert_eq!(shared.rx_bytes(), 0);

    // RX path: device transmit pushes 8 bytes
    let mut device = SmoltcpDevice::new(shared.clone(), 1500);
    let tx_token = device.transmit(Instant::from_millis(0)).unwrap();
    smoltcp::phy::TxToken::consume(tx_token, 8, |buf| {
        buf.fill(0);
    });

    assert_eq!(shared.tx_bytes(), 10);
    assert_eq!(shared.rx_bytes(), 8);
}

/// Test 4: Connection tracker lifecycle — create, check, and cleanup.
#[test]
fn connection_tracker_lifecycle() {
    let mut tracker = ConnectionTracker::new(Some(16));
    let mut sockets = SocketSet::new(vec![]);

    let src = "10.0.2.100:5000".parse().unwrap();
    let dst = "93.184.216.34:80".parse().unwrap();

    // Create a connection
    assert!(!tracker.has_socket_for(&src, &dst));
    assert!(tracker.create_tcp_socket(src, dst, &mut sockets));
    assert!(tracker.has_socket_for(&src, &dst));

    // Socket is in Listen state, no new connections yet
    let new = tracker.take_new_connections(&mut sockets);
    assert!(new.is_empty());

    // Cleanup should not remove non-Closed sockets
    tracker.cleanup_closed(&mut sockets);
    assert!(tracker.has_socket_for(&src, &dst));
}

/// Test 5: Multiple concurrent connections tracked independently.
#[test]
fn multiple_connections_independent_lifecycle() {
    let mut tracker = ConnectionTracker::new(None);
    let mut sockets = SocketSet::new(vec![]);

    let pairs: Vec<(std::net::SocketAddr, std::net::SocketAddr)> = (0..5)
        .map(|i| {
            let src: std::net::SocketAddr = format!("10.0.2.100:{}", 5000 + i).parse().unwrap();
            let dst: std::net::SocketAddr = format!("93.184.216.34:{}", 80 + i).parse().unwrap();
            (src, dst)
        })
        .collect();

    for (src, dst) in &pairs {
        assert!(tracker.create_tcp_socket(*src, *dst, &mut sockets));
    }

    for (src, dst) in &pairs {
        assert!(tracker.has_socket_for(src, dst));
    }

    // Cross-pairs should not exist
    assert!(!tracker.has_socket_for(&pairs[0].0, &pairs[1].1));
}
