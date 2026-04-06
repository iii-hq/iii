//! Integration tests for iii-network.
//!
//! These tests verify that the network subsystem components work together
//! correctly: SharedState queues, SmoltcpBackend frame bridging, SmoltcpDevice
//! frame staging, ConnectionTracker lifecycle management, and the poll loop
//! iteration logic.

use std::net::Ipv4Addr;
use std::sync::Arc;

use iii_network::{
    ConnectionTracker, DnsInterceptor, PollLoopConfig, PollLoopState, SharedState, SmoltcpBackend,
    SmoltcpDevice, UdpRelay, create_interface, poll_iteration,
};
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

// ---------------------------------------------------------------------------
// poll_iteration integration tests
// ---------------------------------------------------------------------------

fn test_config() -> PollLoopConfig {
    PollLoopConfig {
        gateway_mac: [0x02, 0x00, 0x00, 0x00, 0x00, 0x01],
        guest_mac: [0x02, 0x00, 0x00, 0x00, 0x00, 0x02],
        gateway_ipv4: Ipv4Addr::new(10, 0, 2, 1),
        guest_ipv4: Ipv4Addr::new(10, 0, 2, 2),
        mtu: 1500,
    }
}

fn build_state(
    shared: &Arc<SharedState>,
    config: &PollLoopConfig,
    handle: &tokio::runtime::Handle,
) -> PollLoopState {
    let mut device = SmoltcpDevice::new(shared.clone(), config.mtu);
    let iface = create_interface(&mut device, config);
    let mut sockets = SocketSet::new(vec![]);
    let dns = DnsInterceptor::new(&mut sockets, shared.clone(), handle);
    let udp = UdpRelay::new(
        shared.clone(),
        config.gateway_mac,
        config.guest_mac,
        handle.clone(),
    );
    PollLoopState {
        device,
        iface,
        sockets,
        conn_tracker: ConnectionTracker::new(None),
        dns_interceptor: dns,
        udp_relay: udp,
        last_cleanup: std::time::Instant::now(),
    }
}

fn build_tcp_syn_frame(src_ip: [u8; 4], dst_ip: [u8; 4], src_port: u16, dst_port: u16) -> Vec<u8> {
    let mut frame = vec![0u8; 14 + 20 + 20];
    frame[12] = 0x08;
    frame[13] = 0x00;

    let ip = &mut frame[14..34];
    ip[0] = 0x45;
    ip[2..4].copy_from_slice(&40u16.to_be_bytes());
    ip[6] = 0x40;
    ip[8] = 64;
    ip[9] = 6;
    ip[12..16].copy_from_slice(&src_ip);
    ip[16..20].copy_from_slice(&dst_ip);

    let tcp = &mut frame[34..54];
    tcp[0..2].copy_from_slice(&src_port.to_be_bytes());
    tcp[2..4].copy_from_slice(&dst_port.to_be_bytes());
    tcp[12] = 0x50;
    tcp[13] = 0x02;
    frame
}

fn build_udp_frame(src_ip: [u8; 4], dst_ip: [u8; 4], src_port: u16, dst_port: u16) -> Vec<u8> {
    let mut frame = vec![0u8; 14 + 20 + 8];
    frame[12] = 0x08;
    frame[13] = 0x00;

    let ip = &mut frame[14..34];
    ip[0] = 0x45;
    ip[2..4].copy_from_slice(&28u16.to_be_bytes());
    ip[8] = 64;
    ip[9] = 17;
    ip[12..16].copy_from_slice(&src_ip);
    ip[16..20].copy_from_slice(&dst_ip);

    let udp = &mut frame[34..42];
    udp[0..2].copy_from_slice(&src_port.to_be_bytes());
    udp[2..4].copy_from_slice(&dst_port.to_be_bytes());
    udp[4..6].copy_from_slice(&8u16.to_be_bytes());
    frame
}

/// poll_iteration with an empty tx_ring produces no side effects.
#[tokio::test]
async fn poll_iteration_empty_queue() {
    let shared = Arc::new(SharedState::new(64));
    let config = test_config();
    let handle = tokio::runtime::Handle::current();
    let mut state = build_state(&shared, &config, &handle);

    let result = poll_iteration(&mut state, &shared, &config, &handle);
    assert!(!result.frames_emitted);
    assert_eq!(result.new_connections, 0);
}

/// TCP SYN injected into SharedState is picked up by poll_iteration and
/// creates a tracked connection in the connection tracker.
#[tokio::test]
async fn poll_iteration_syn_creates_tracked_connection() {
    let shared = Arc::new(SharedState::new(64));
    let config = test_config();
    let handle = tokio::runtime::Handle::current();
    let mut state = build_state(&shared, &config, &handle);

    let syn = build_tcp_syn_frame([10, 0, 2, 2], [93, 184, 216, 34], 40000, 443);
    shared.tx_ring.push(syn).unwrap();

    let src: std::net::SocketAddr = "10.0.2.2:40000".parse().unwrap();
    let dst: std::net::SocketAddr = "93.184.216.34:443".parse().unwrap();

    let _result = poll_iteration(&mut state, &shared, &config, &handle);
    assert!(
        state.conn_tracker.has_socket_for(&src, &dst),
        "SYN should register a connection"
    );
}

/// DNS frame (UDP to port 53) does not create a TCP connection.
#[tokio::test]
async fn poll_iteration_dns_no_tcp_connection() {
    let shared = Arc::new(SharedState::new(64));
    let config = test_config();
    let handle = tokio::runtime::Handle::current();
    let mut state = build_state(&shared, &config, &handle);

    let dns = build_udp_frame([10, 0, 2, 2], [10, 0, 2, 1], 12345, 53);
    shared.tx_ring.push(dns).unwrap();

    let result = poll_iteration(&mut state, &shared, &config, &handle);
    assert_eq!(result.new_connections, 0);
}

/// Multiple SYN frames in a single iteration create independent connections.
#[tokio::test]
async fn poll_iteration_multiple_syns() {
    let shared = Arc::new(SharedState::new(64));
    let config = test_config();
    let handle = tokio::runtime::Handle::current();
    let mut state = build_state(&shared, &config, &handle);

    for port in 50000..50003u16 {
        let syn = build_tcp_syn_frame([10, 0, 2, 2], [1, 1, 1, 1], port, 80);
        shared.tx_ring.push(syn).unwrap();
    }

    let _result = poll_iteration(&mut state, &shared, &config, &handle);

    for port in 50000..50003u16 {
        let src: std::net::SocketAddr = format!("10.0.2.2:{}", port).parse().unwrap();
        let dst: std::net::SocketAddr = "1.1.1.1:80".parse().unwrap();
        assert!(
            state.conn_tracker.has_socket_for(&src, &dst),
            "connection for port {} should exist",
            port
        );
    }
}

/// Cleanup runs when last_cleanup is old enough.
#[tokio::test]
async fn poll_iteration_cleanup_runs_after_interval() {
    let shared = Arc::new(SharedState::new(64));
    let config = test_config();
    let handle = tokio::runtime::Handle::current();
    let mut state = build_state(&shared, &config, &handle);

    state.last_cleanup = std::time::Instant::now() - std::time::Duration::from_secs(2);
    let old_cleanup = state.last_cleanup;

    let _result = poll_iteration(&mut state, &shared, &config, &handle);

    assert!(
        state.last_cleanup > old_cleanup,
        "last_cleanup should be updated after >= 1s elapsed"
    );
}
