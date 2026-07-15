//! Integration tests for iii-network.
//!
//! These tests verify that the network subsystem components work together
//! correctly: SharedState queues, SmoltcpBackend frame bridging, SmoltcpDevice
//! frame staging, ConnectionTracker lifecycle management, and the poll loop
//! iteration logic.

use std::net::Ipv4Addr;
use std::sync::Arc;

use iii_network::{
    ConnectionTracker, DnsInterceptor, FrameAction, NewConnection, PollLoopConfig, PollLoopState,
    SharedState, SmoltcpBackend, SmoltcpDevice, UdpRelay, classify_frame, create_interface,
    poll_iteration,
};
use msb_krun::backends::net::NetBackend;
use smoltcp::iface::{PollResult, SocketSet};
use smoltcp::phy::{ChecksumCapabilities, Device};
use smoltcp::time::Instant;
use smoltcp::wire::{
    ArpOperation, ArpPacket, ArpRepr, EthernetAddress, EthernetFrame, EthernetProtocol,
    EthernetRepr, IpProtocol, Ipv4Packet, Ipv4Repr, TcpControl, TcpPacket, TcpRepr, TcpSeqNumber,
};

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

// ---------------------------------------------------------------------------
// MOT-3966: wake-pipe reliability soak
// ---------------------------------------------------------------------------

/// The rx_wake pipe must never accumulate bytes across a long-lived VM's
/// traffic: the msb_krun NetWorker epolls it EDGE_TRIGGERED and never reads
/// it, so an ever-filling pipe eventually swallows every wake (16 KiB budget
/// on macOS) and host→guest delivery dies permanently.
#[test]
fn soak_wake_pipe_survives_100k_frames() {
    let shared = Arc::new(SharedState::new(64));
    let mut backend = SmoltcpBackend::new(shared.clone());
    let mut buf = vec![0u8; 256];
    let mut delivered = 0u32;

    for i in 0..100_000u32 {
        shared.rx_ring.push(i.to_be_bytes().to_vec()).unwrap();
        shared.rx_wake.wake();
        // Mimic msb_krun's process_rx: loop read_frame until NothingRead.
        while backend.read_frame(&mut buf).is_ok() {
            delivered += 1;
        }
    }

    assert_eq!(delivered, 100_000);

    // The pipe must end drained, not accumulating toward saturation.
    let mut b = [0u8; 512];
    // SAFETY: valid nonblocking pipe read end, local buffer.
    let n = unsafe { libc::read(shared.rx_wake.as_raw_fd(), b.as_mut_ptr().cast(), b.len()) };
    assert!(n < 0, "wake pipe accumulated {n} bytes over the soak");
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

// ---------------------------------------------------------------------------
// MOT-3966: data-pump correctness tests with a seq-aware in-test TCP guest
// ---------------------------------------------------------------------------

/// Minimal TCP client living "inside the guest": builds checksummed
/// Ethernet/IPv4/TCP frames, answers ARP, tracks seq/ack/window, and
/// collects payload bytes the gateway sends back.
struct TestGuest {
    guest_mac: EthernetAddress,
    gateway_mac: EthernetAddress,
    guest_ip: Ipv4Addr,
    gateway_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
    src_port: u16,
    dst_port: u16,
    /// Next sequence number we send.
    seq: u32,
    /// Next byte we expect from the peer (valid once established).
    ack: u32,
    /// Peer's advertised receive window.
    peer_window: u16,
    /// Highest ack number the peer has sent (bytes of ours it has).
    peer_acked: u32,
    established: bool,
    peer_fin: bool,
    received: Vec<u8>,
}

impl TestGuest {
    fn new(config: &PollLoopConfig, dst_ip: Ipv4Addr, src_port: u16, dst_port: u16) -> Self {
        Self {
            guest_mac: EthernetAddress(config.guest_mac),
            gateway_mac: EthernetAddress(config.gateway_mac),
            guest_ip: config.guest_ipv4,
            gateway_ip: config.gateway_ipv4,
            dst_ip,
            src_port,
            dst_port,
            seq: 1000,
            ack: 0,
            peer_window: 0,
            peer_acked: 1000,
            established: false,
            peer_fin: false,
            received: Vec::new(),
        }
    }

    /// Bytes in flight (sent but not yet acked by the peer).
    fn in_flight(&self) -> u32 {
        self.seq.wrapping_sub(self.peer_acked)
    }

    /// Gratuitous ARP request so smoltcp learns our MAC before replying —
    /// avoids the SYN-ACK being parked on neighbor resolution.
    fn arp_announce(&self) -> Vec<u8> {
        let arp = ArpRepr::EthernetIpv4 {
            operation: ArpOperation::Request,
            source_hardware_addr: self.guest_mac,
            source_protocol_addr: self.guest_ip,
            target_hardware_addr: EthernetAddress::BROADCAST,
            target_protocol_addr: self.gateway_ip,
        };
        let eth = EthernetRepr {
            src_addr: self.guest_mac,
            dst_addr: EthernetAddress::BROADCAST,
            ethertype: EthernetProtocol::Arp,
        };
        let mut buf = vec![0u8; 14 + arp.buffer_len()];
        eth.emit(&mut EthernetFrame::new_unchecked(&mut buf));
        arp.emit(&mut ArpPacket::new_unchecked(&mut buf[14..]));
        buf
    }

    fn tcp_frame(&mut self, control: TcpControl, payload: &[u8]) -> Vec<u8> {
        let tcp = TcpRepr {
            src_port: self.src_port,
            dst_port: self.dst_port,
            control,
            seq_number: TcpSeqNumber(self.seq as i32),
            ack_number: self.established.then_some(TcpSeqNumber(self.ack as i32)),
            window_len: 65535,
            window_scale: None,
            max_seg_size: (control == TcpControl::Syn).then_some(1460),
            sack_permitted: false,
            sack_ranges: [None, None, None],
            timestamp: None,
            payload,
        };
        let ip = Ipv4Repr {
            src_addr: self.guest_ip,
            dst_addr: self.dst_ip,
            next_header: IpProtocol::Tcp,
            payload_len: tcp.buffer_len(),
            hop_limit: 64,
        };
        let eth = EthernetRepr {
            src_addr: self.guest_mac,
            dst_addr: self.gateway_mac,
            ethertype: EthernetProtocol::Ipv4,
        };

        let mut buf = vec![0u8; 14 + ip.buffer_len() + tcp.buffer_len()];
        let caps = ChecksumCapabilities::default();
        eth.emit(&mut EthernetFrame::new_unchecked(&mut buf));
        ip.emit(&mut Ipv4Packet::new_unchecked(&mut buf[14..]), &caps);
        tcp.emit(
            &mut TcpPacket::new_unchecked(&mut buf[14 + ip.buffer_len()..]),
            &self.guest_ip.into(),
            &self.dst_ip.into(),
            &caps,
        );

        self.seq = self
            .seq
            .wrapping_add(payload.len() as u32)
            .wrapping_add(matches!(control, TcpControl::Syn | TcpControl::Fin) as u32);
        buf
    }

    fn syn(&mut self) -> Vec<u8> {
        self.tcp_frame(TcpControl::Syn, &[])
    }

    /// Retransmit the initial SYN with the same ISN, like a kernel SYN
    /// retry. Only valid before any data has been sent.
    fn retransmit_syn(&mut self) -> Vec<u8> {
        let after = self.seq;
        self.seq = after.wrapping_sub(1);
        let frame = self.tcp_frame(TcpControl::Syn, &[]);
        debug_assert_eq!(self.seq, after);
        frame
    }

    fn data(&mut self, payload: &[u8]) -> Vec<u8> {
        self.tcp_frame(TcpControl::Psh, payload)
    }

    fn fin(&mut self) -> Vec<u8> {
        self.tcp_frame(TcpControl::Fin, &[])
    }

    fn pure_ack(&mut self) -> Vec<u8> {
        self.tcp_frame(TcpControl::None, &[])
    }

    /// Consume every frame the gateway emitted; update seq/ack/window state,
    /// collect payloads, and queue ARP replies / pure ACKs back into tx_ring.
    fn drain_egress(&mut self, shared: &SharedState) {
        let mut ack_due = false;

        while let Some(frame) = shared.rx_ring.pop() {
            let Ok(eth) = EthernetFrame::new_checked(&frame[..]) else {
                continue;
            };
            match eth.ethertype() {
                EthernetProtocol::Arp => {
                    let Ok(arp) = ArpPacket::new_checked(eth.payload()) else {
                        continue;
                    };
                    if arp.operation() == ArpOperation::Request {
                        let reply = ArpRepr::EthernetIpv4 {
                            operation: ArpOperation::Reply,
                            source_hardware_addr: self.guest_mac,
                            source_protocol_addr: self.guest_ip,
                            target_hardware_addr: self.gateway_mac,
                            target_protocol_addr: self.gateway_ip,
                        };
                        let eth_out = EthernetRepr {
                            src_addr: self.guest_mac,
                            dst_addr: self.gateway_mac,
                            ethertype: EthernetProtocol::Arp,
                        };
                        let mut buf = vec![0u8; 14 + reply.buffer_len()];
                        eth_out.emit(&mut EthernetFrame::new_unchecked(&mut buf));
                        reply.emit(&mut ArpPacket::new_unchecked(&mut buf[14..]));
                        shared.tx_ring.push(buf).unwrap();
                    }
                }
                EthernetProtocol::Ipv4 => {
                    let Ok(ip) = Ipv4Packet::new_checked(eth.payload()) else {
                        continue;
                    };
                    if ip.next_header() != IpProtocol::Tcp {
                        continue;
                    }
                    let (src_addr, dst_addr) = (ip.src_addr(), ip.dst_addr());
                    let Ok(tcp_pkt) = TcpPacket::new_checked(ip.payload()) else {
                        continue;
                    };
                    let Ok(tcp) = TcpRepr::parse(
                        &tcp_pkt,
                        &src_addr.into(),
                        &dst_addr.into(),
                        &ChecksumCapabilities::default(),
                    ) else {
                        continue;
                    };
                    if tcp.dst_port != self.src_port {
                        continue;
                    }

                    assert!(
                        tcp.control != TcpControl::Rst,
                        "gateway sent an unexpected RST"
                    );

                    if let Some(ack) = tcp.ack_number {
                        self.peer_acked = ack.0 as u32;
                    }
                    self.peer_window = tcp.window_len;

                    if tcp.control == TcpControl::Syn {
                        // SYN-ACK: complete the handshake.
                        self.ack = (tcp.seq_number.0 as u32).wrapping_add(1);
                        self.established = true;
                        ack_due = true;
                    } else {
                        if !tcp.payload.is_empty() {
                            self.received.extend_from_slice(tcp.payload);
                            self.ack = self.ack.wrapping_add(tcp.payload.len() as u32);
                            ack_due = true;
                        }
                        if tcp.control == TcpControl::Fin {
                            self.peer_fin = true;
                            self.ack = self.ack.wrapping_add(1);
                            ack_due = true;
                        }
                    }
                }
                _ => {}
            }
        }

        if ack_due {
            let ack = self.pure_ack();
            shared.tx_ring.push(ack).unwrap();
        }
    }
}

/// Monotonic smoltcp clock for the proxy-less poll helper.
fn test_now() -> Instant {
    static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    Instant::from_millis(
        EPOCH
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_millis() as i64,
    )
}

/// One poll-loop pass that does everything `poll_iteration` does for TCP
/// EXCEPT spawning real proxy tasks — new connections are returned to the
/// caller, which acts as the proxy (deterministic backpressure control).
fn poll_no_proxy(state: &mut PollLoopState) -> Vec<NewConnection> {
    let now = test_now();

    while let Some(frame) = state.device.stage_next_frame() {
        if let FrameAction::TcpSyn { src, dst } = classify_frame(frame) {
            if !state.conn_tracker.has_socket_for(&src, &dst) {
                state
                    .conn_tracker
                    .create_tcp_socket(src, dst, &mut state.sockets);
            }
        }
        state
            .iface
            .poll_ingress_single(now, &mut state.device, &mut state.sockets);
    }

    while !matches!(
        state
            .iface
            .poll_egress(now, &mut state.device, &mut state.sockets),
        PollResult::None
    ) {}

    state.conn_tracker.relay_data(&mut state.sockets);
    let new = state.conn_tracker.take_new_connections(&mut state.sockets);

    while !matches!(
        state
            .iface
            .poll_egress(now, &mut state.device, &mut state.sockets),
        PollResult::None
    ) {}

    new
}

/// S1 (MOT-3966): a full `to_proxy` channel must NOT lose bytes. Pre-fix,
/// `relay_data` dequeued from the socket (already ACKed to the guest) and
/// dropped the chunk when `try_send` failed — a permanent hole in the
/// stream. Post-fix the bytes stay in the socket buffer and the window
/// closes; everything is delivered once the channel drains.
#[tokio::test]
async fn relay_backpressure_loses_no_bytes() {
    let shared = Arc::new(SharedState::new(2048));
    let config = test_config();
    let handle = tokio::runtime::Handle::current();
    let mut state = build_state(&shared, &config, &handle);

    let dst_ip = Ipv4Addr::new(93, 184, 216, 34);
    let mut guest = TestGuest::new(&config, dst_ip, 41000, 9999);

    // ARP + handshake.
    shared.tx_ring.push(guest.arp_announce()).unwrap();
    poll_no_proxy(&mut state);
    let syn = guest.syn();
    shared.tx_ring.push(syn).unwrap();
    let mut conn = None;
    for _ in 0..20 {
        let mut new = poll_no_proxy(&mut state);
        guest.drain_egress(&shared);
        if let Some(c) = new.pop() {
            conn = Some(c);
        }
        if guest.established && conn.is_some() {
            break;
        }
    }
    let NewConnection {
        mut from_smoltcp,
        to_smoltcp: _to_smoltcp,
        ..
    } = conn.expect("connection should be established and handed to the proxy");

    // Phase 1: pump data WITHOUT consuming the channel until the advertised
    // window stays closed (socket buffer full ⇒ channel full: 32 slots).
    let total: usize = 900 * 1024;
    let pattern = |i: usize| (i % 251) as u8;
    let mut sent = 0usize;
    let mut zero_window_polls = 0u32;
    let mut safety = 0u32;

    while sent < total && zero_window_polls < 5 {
        safety += 1;
        assert!(safety < 20_000, "phase 1 did not converge");

        let window = guest.peer_window as usize;
        let in_flight = guest.in_flight() as usize;
        let budget = window.saturating_sub(in_flight);
        if budget == 0 {
            poll_no_proxy(&mut state);
            guest.drain_egress(&shared);
            if guest.peer_window == 0 && guest.in_flight() == 0 {
                zero_window_polls += 1;
            }
            continue;
        }
        zero_window_polls = 0;

        let len = budget.min(1460).min(total - sent);
        let payload: Vec<u8> = (sent..sent + len).map(pattern).collect();
        let frame = guest.data(&payload);
        shared.tx_ring.push(frame).unwrap();
        sent += len;

        poll_no_proxy(&mut state);
        guest.drain_egress(&shared);
    }

    assert!(
        zero_window_polls >= 5 || sent == total,
        "expected either sustained zero-window backpressure or full send"
    );

    // Phase 2: act as the proxy — drain the channel while pumping the rest.
    let mut delivered: Vec<u8> = Vec::with_capacity(total);
    let mut idle = 0u32;
    while delivered.len() < sent || sent < total {
        while let Ok(chunk) = from_smoltcp.try_recv() {
            delivered.extend_from_slice(&chunk);
        }

        if sent < total {
            let window = guest.peer_window as usize;
            let in_flight = guest.in_flight() as usize;
            let budget = window.saturating_sub(in_flight);
            if budget > 0 {
                let len = budget.min(1460).min(total - sent);
                let payload: Vec<u8> = (sent..sent + len).map(pattern).collect();
                let frame = guest.data(&payload);
                shared.tx_ring.push(frame).unwrap();
                sent += len;
                idle = 0;
            }
        }

        poll_no_proxy(&mut state);
        guest.drain_egress(&shared);

        idle += 1;
        assert!(
            idle < 50_000,
            "phase 2 stalled: {}/{} bytes",
            delivered.len(),
            sent
        );
    }

    assert_eq!(sent, total);
    assert_eq!(
        delivered.len(),
        total,
        "bytes were lost in the relay under backpressure"
    );
    for (i, &b) in delivered.iter().enumerate() {
        assert_eq!(b, pattern(i), "stream corrupted at offset {i}");
    }
}

/// Poll + let proxy tasks run, until `done` or the deadline expires.
async fn pump_until(
    state: &mut PollLoopState,
    shared: &Arc<SharedState>,
    config: &PollLoopConfig,
    handle: &tokio::runtime::Handle,
    guest: &mut TestGuest,
    mut done: impl FnMut(&TestGuest, &PollLoopState) -> bool,
    what: &str,
) {
    for _ in 0..2000 {
        poll_iteration(state, shared, config, handle);
        guest.drain_egress(shared);
        if done(guest, state) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    panic!("timed out waiting for: {what}");
}

/// S2+S3 (MOT-3966): a guest that sends ACK+request+FIN inside ONE poll
/// batch is first observed past Established (CloseWait). The proxy must
/// still spawn, deliver the request, propagate the FIN to the server (EOF),
/// relay the response back, and the tracker entry must be reaped.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fast_closing_connection_still_proxied_and_reaped() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        use std::io::{Read, Write};
        let (mut sock, _) = listener.accept().unwrap();
        let mut req = Vec::new();
        // Returns only on EOF — requires the guest FIN to be propagated.
        sock.read_to_end(&mut req).unwrap();
        sock.write_all(b"pong").unwrap();
        req
    });

    let shared = Arc::new(SharedState::new(2048));
    let config = test_config();
    let handle = tokio::runtime::Handle::current();
    let mut state = build_state(&shared, &config, &handle);

    // dst = gateway ⇒ the proxy rewrites to 127.0.0.1:port.
    let mut guest = TestGuest::new(&config, config.gateway_ipv4, 42000, port);

    shared.tx_ring.push(guest.arp_announce()).unwrap();
    poll_iteration(&mut state, &shared, &config, &handle);

    // SYN → SYN-ACK.
    let syn = guest.syn();
    shared.tx_ring.push(syn).unwrap();
    pump_until(
        &mut state,
        &shared,
        &config,
        &handle,
        &mut guest,
        |g, _| g.established,
        "handshake",
    )
    .await;

    // Drop the handshake ACK, request, and FIN into ONE batch: the tracker
    // first sees this socket in CloseWait.
    let ack = guest.pure_ack();
    let data = guest.data(b"ping");
    let fin = guest.fin();
    shared.tx_ring.push(ack).unwrap();
    shared.tx_ring.push(data).unwrap();
    shared.tx_ring.push(fin).unwrap();

    pump_until(
        &mut state,
        &shared,
        &config,
        &handle,
        &mut guest,
        |g, _| g.received == b"pong" && g.peer_fin,
        "response + FIN from the gateway",
    )
    .await;

    let req = server.join().unwrap();
    assert_eq!(req, b"ping", "server must receive the request bytes");

    // The connection must be fully reaped (socket Closed, key released).
    let src: std::net::SocketAddr = format!("{}:42000", config.guest_ipv4).parse().unwrap();
    let dst: std::net::SocketAddr = format!("{}:{}", config.gateway_ipv4, port).parse().unwrap();
    state.last_cleanup = std::time::Instant::now() - std::time::Duration::from_secs(2);
    pump_until(
        &mut state,
        &shared,
        &config,
        &handle,
        &mut guest,
        |_, s| !s.conn_tracker.has_socket_for(&src, &dst),
        "connection reaped after close",
    )
    .await;
}

/// S3 (MOT-3966): guest half-close on a long-lived connection reaches the
/// server as EOF while the response path stays open.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn guest_fin_propagates_as_server_eof() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (eof_tx, eof_rx) = std::sync::mpsc::channel::<()>();
    let server = std::thread::spawn(move || {
        use std::io::{Read, Write};
        let (mut sock, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4];
        sock.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"ping");
        // Next read must observe EOF once the guest FINs.
        let n = sock.read(&mut [0u8; 16]).unwrap();
        assert_eq!(n, 0, "expected EOF from the propagated guest FIN");
        eof_tx.send(()).unwrap();
        sock.write_all(b"pong").unwrap();
    });

    let shared = Arc::new(SharedState::new(2048));
    let config = test_config();
    let handle = tokio::runtime::Handle::current();
    let mut state = build_state(&shared, &config, &handle);
    let mut guest = TestGuest::new(&config, config.gateway_ipv4, 43000, port);

    shared.tx_ring.push(guest.arp_announce()).unwrap();
    poll_iteration(&mut state, &shared, &config, &handle);

    let syn = guest.syn();
    shared.tx_ring.push(syn).unwrap();
    pump_until(
        &mut state,
        &shared,
        &config,
        &handle,
        &mut guest,
        |g, _| g.established,
        "handshake",
    )
    .await;

    let data = guest.data(b"ping");
    shared.tx_ring.push(data).unwrap();

    // Let the request flow, then half-close from the guest.
    pump_until(
        &mut state,
        &shared,
        &config,
        &handle,
        &mut guest,
        |g, _| g.in_flight() == 0,
        "request acked",
    )
    .await;
    let fin = guest.fin();
    shared.tx_ring.push(fin).unwrap();

    pump_until(
        &mut state,
        &shared,
        &config,
        &handle,
        &mut guest,
        |g, _| g.received == b"pong",
        "response after half-close",
    )
    .await;

    eof_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("server never observed EOF — guest FIN was not propagated");
    server.join().unwrap();
}

/// MOT-3966 first-boot wedge repro: the engine's listener isn't up yet when
/// the guest first dials, so the proxy dial is REFUSED and the stack closes
/// the guest connection. The guest then reconnects from the SAME source port
/// (Linux reuses the ephemeral port for the same destination once the prior
/// socket is closed). The stale tracker entry pins the (src,dst) key, the
/// retry SYN is swallowed by the lingering smoltcp socket, and — the field
/// wedge — once the connection finally establishes, its bytes must still
/// reach the (now-listening) server.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn refused_dial_then_same_port_reconnect_delivers_data() {
    // Reserve a port with no listener: bind, capture, drop. The first dial
    // gets ECONNREFUSED, exactly like the engine's listener racing VM spawn.
    let placeholder = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = placeholder.local_addr().unwrap().port();
    drop(placeholder);

    let shared = Arc::new(SharedState::new(2048));
    let config = test_config();
    let handle = tokio::runtime::Handle::current();
    let mut state = build_state(&shared, &config, &handle);

    const SRC_PORT: u16 = 44100;

    // --- Attempt A: establishes guest-side, proxy dial refused, stack
    // closes toward the guest, guest completes the close handshake. ---
    let mut guest_a = TestGuest::new(&config, config.gateway_ipv4, SRC_PORT, port);
    shared.tx_ring.push(guest_a.arp_announce()).unwrap();
    poll_iteration(&mut state, &shared, &config, &handle);

    let syn = guest_a.syn();
    shared.tx_ring.push(syn).unwrap();
    pump_until(
        &mut state,
        &shared,
        &config,
        &handle,
        &mut guest_a,
        |g, _| g.established,
        "attempt A handshake",
    )
    .await;

    // Guest ships its request immediately (the WS upgrade in the field).
    let ack = guest_a.pure_ack();
    let data = guest_a.data(b"attempt-a upgrade");
    shared.tx_ring.push(ack).unwrap();
    shared.tx_ring.push(data).unwrap();

    // Refused dial → proxy dies → relay closes → guest sees FIN.
    pump_until(
        &mut state,
        &shared,
        &config,
        &handle,
        &mut guest_a,
        |g, _| g.peer_fin,
        "FIN from refused dial",
    )
    .await;
    // Guest closes too (its FIN), mirroring the app closing on EOF.
    let fin = guest_a.fin();
    shared.tx_ring.push(fin).unwrap();
    poll_iteration(&mut state, &shared, &config, &handle);
    guest_a.drain_egress(&shared);

    // --- Engine "comes up": bind the real listener now. ---
    let listener = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
    let server = std::thread::spawn(move || {
        use std::io::Read;
        let (mut sock, _) = listener.accept().unwrap();
        let mut req = Vec::new();
        sock.read_to_end(&mut req).unwrap();
        req
    });

    // --- Attempt B: same (src,dst) tuple, like Linux port reuse. ---
    let mut guest_b = TestGuest::new(&config, config.gateway_ipv4, SRC_PORT, port);
    // Fresh kernel connection: a new (random) ISN, unrelated to attempt A's.
    guest_b.seq = 900_000;
    guest_b.peer_acked = 900_000;
    let syn = guest_b.syn();
    shared.tx_ring.push(syn).unwrap();

    // Drive with SYN retransmits like a real kernel: the stale entry pins
    // the tuple while the old socket lingers (TIME-WAIT is 10s in smoltcp),
    // so keep retrying for up to ~15s of wall time.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let mut next_retransmit = std::time::Instant::now() + std::time::Duration::from_secs(1);
    while !guest_b.established {
        assert!(
            std::time::Instant::now() < deadline,
            "attempt B never established: retry SYN swallowed forever"
        );
        // Force cleanup cadence like the real loop.
        state.last_cleanup = std::time::Instant::now() - std::time::Duration::from_secs(2);
        poll_iteration(&mut state, &shared, &config, &handle);
        guest_b.drain_egress(&shared);
        if std::time::Instant::now() >= next_retransmit && !guest_b.established {
            let rt = guest_b.retransmit_syn();
            shared.tx_ring.push(rt).unwrap();
            next_retransmit = std::time::Instant::now() + std::time::Duration::from_secs(1);
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    // The wedge: these bytes must reach the server.
    let ack = guest_b.pure_ack();
    let data = guest_b.data(b"attempt-b upgrade");
    shared.tx_ring.push(ack).unwrap();
    shared.tx_ring.push(data).unwrap();
    let fin_deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut sent_fin = false;
    loop {
        assert!(
            std::time::Instant::now() < fin_deadline,
            "attempt B data was never delivered/acked (the MOT-3966 wedge)"
        );
        poll_iteration(&mut state, &shared, &config, &handle);
        guest_b.drain_egress(&shared);
        if guest_b.in_flight() == 0 && !sent_fin {
            // Data acked — close so the server's read_to_end returns.
            let fin = guest_b.fin();
            shared.tx_ring.push(fin).unwrap();
            sent_fin = true;
        }
        if sent_fin && guest_b.peer_fin {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let req = server.join().unwrap();
    assert_eq!(
        req, b"attempt-b upgrade",
        "server must receive attempt B's bytes"
    );
}
