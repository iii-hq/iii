//! smoltcp interface setup, frame classification, and poll loop.
//!
//! This module contains the core networking event loop that runs on a
//! dedicated OS thread. It bridges guest ethernet frames (via
//! [`SmoltcpDevice`]) to smoltcp's TCP/IP stack and services connections
//! through tokio proxy tasks.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::time::Instant;
use smoltcp::wire::{
    EthernetAddress, EthernetFrame, EthernetProtocol, HardwareAddress, IpAddress, IpCidr,
    IpProtocol, Ipv4Packet, TcpPacket, UdpPacket,
};

use crate::conn::ConnectionTracker;
use crate::device::SmoltcpDevice;
use crate::dns::DnsInterceptor;
use crate::proxy;
use crate::shared::SharedState;
use crate::udp_relay::UdpRelay;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Result of classifying a guest ethernet frame before smoltcp processes it.
///
/// Pre-inspection allows the poll loop to:
/// - Create TCP sockets before smoltcp sees a SYN (preventing auto-RST).
/// - Handle non-DNS UDP outside smoltcp (Phase 7).
/// - Route DNS queries to the interception handler (Phase 7).
pub enum FrameAction {
    /// TCP SYN to a new destination — create a smoltcp socket before
    /// letting smoltcp process the frame.
    TcpSyn { src: SocketAddr, dst: SocketAddr },

    /// Non-DNS UDP datagram — handled outside smoltcp via UDP relay (Phase 7).
    UdpRelay { src: SocketAddr, dst: SocketAddr },

    /// DNS query (UDP to port 53) — handled by DNS interceptor (Phase 7).
    Dns,

    /// Everything else (ARP, TCP data/ACK/FIN, etc.) — let smoltcp process.
    Passthrough,
}

/// Resolved network parameters for the poll loop. Created by
/// `SmoltcpNetwork::new()` from `NetworkConfig` + sandbox slot.
pub struct PollLoopConfig {
    pub gateway_mac: [u8; 6],
    pub guest_mac: [u8; 6],
    pub gateway_ipv4: Ipv4Addr,
    pub guest_ipv4: Ipv4Addr,
    pub mtu: usize,
}

/// Mutable state owned by the poll loop, factored out for testability.
pub struct PollLoopState {
    pub device: SmoltcpDevice,
    pub iface: Interface,
    pub sockets: SocketSet<'static>,
    pub conn_tracker: ConnectionTracker,
    pub dns_interceptor: DnsInterceptor,
    pub udp_relay: UdpRelay,
    pub last_cleanup: std::time::Instant,
}

/// Summary of work done in a single poll iteration.
pub struct IterationResult {
    /// Whether any frames were emitted to the rx_ring during this iteration.
    pub frames_emitted: bool,
    /// Number of new TCP connections ready for proxy spawning.
    pub new_connections: usize,
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Classify a raw ethernet frame for pre-inspection.
///
/// Uses smoltcp's wire module for zero-copy parsing. Returns
/// [`FrameAction::Passthrough`] for any frame that cannot be parsed or
/// doesn't match a special case.
pub fn classify_frame(frame: &[u8]) -> FrameAction {
    let Ok(eth) = EthernetFrame::new_checked(frame) else {
        return FrameAction::Passthrough;
    };

    match eth.ethertype() {
        EthernetProtocol::Ipv4 => classify_ipv4(eth.payload()),
        _ => FrameAction::Passthrough,
    }
}

/// Create and configure the smoltcp [`Interface`].
///
/// The interface is configured as the **gateway**: it owns the gateway IP
/// addresses and responds to ARP for them. `any_ip` mode is enabled so
/// smoltcp accepts traffic destined for arbitrary remote IPs.
pub fn create_interface(device: &mut SmoltcpDevice, config: &PollLoopConfig) -> Interface {
    let hw_addr = HardwareAddress::Ethernet(EthernetAddress(config.gateway_mac));
    let iface_config = Config::new(hw_addr);
    let mut iface = Interface::new(iface_config, device, smoltcp_now());

    iface.update_ip_addrs(|addrs| {
        addrs
            .push(IpCidr::new(IpAddress::from(config.gateway_ipv4), 30))
            .expect("failed to add gateway IPv4 address");
    });

    iface
        .routes_mut()
        .add_default_ipv4_route(config.gateway_ipv4.into())
        .expect("failed to add default IPv4 route");

    iface.set_any_ip(true);

    iface
}

/// Run one iteration of the poll loop (phases 1–3 + final egress flush).
///
/// This function is factored out of [`smoltcp_poll_loop`] for testability.
/// The caller is responsible for sleeping (`libc::poll`) and draining wake
/// pipes between iterations.
pub fn poll_iteration(
    state: &mut PollLoopState,
    shared: &Arc<SharedState>,
    config: &PollLoopConfig,
    tokio_handle: &tokio::runtime::Handle,
) -> IterationResult {
    let now = smoltcp_now();
    let mut emitted = false;

    // Phase 1: Drain all guest frames with pre-inspection.
    while let Some(frame) = state.device.stage_next_frame() {
        match classify_frame(frame) {
            FrameAction::TcpSyn { src, dst } => {
                if !state.conn_tracker.has_socket_for(&src, &dst) {
                    state
                        .conn_tracker
                        .create_tcp_socket(src, dst, &mut state.sockets);
                }
                state
                    .iface
                    .poll_ingress_single(now, &mut state.device, &mut state.sockets);
            }
            FrameAction::UdpRelay { src, dst } => {
                state.udp_relay.relay_outbound(frame, src, dst);
                state.device.drop_staged_frame();
            }
            FrameAction::Dns | FrameAction::Passthrough => {
                state
                    .iface
                    .poll_ingress_single(now, &mut state.device, &mut state.sockets);
            }
        }
    }

    // Phase 2: Egress + maintenance.
    drain_egress(now, &mut state.iface, &mut state.device, &mut state.sockets);
    state.iface.poll_maintenance(now);

    if state.device.frames_emitted.swap(false, Ordering::Relaxed) {
        shared.rx_wake.wake();
        emitted = true;
    }

    // Phase 3: Service connections.
    state.conn_tracker.relay_data(&mut state.sockets);
    state.dns_interceptor.process(&mut state.sockets);

    let new_conns = state.conn_tracker.take_new_connections(&mut state.sockets);
    let new_conn_count = new_conns.len();
    for conn in new_conns {
        proxy::spawn_tcp_proxy(
            tokio_handle,
            conn.dst,
            conn.from_smoltcp,
            conn.to_smoltcp,
            shared.clone(),
            config.gateway_ipv4,
        );
    }

    if state.last_cleanup.elapsed() >= std::time::Duration::from_secs(1) {
        state.conn_tracker.cleanup_closed(&mut state.sockets);
        state.udp_relay.cleanup_expired();
        state.last_cleanup = std::time::Instant::now();
    }

    // Final egress flush.
    drain_egress(now, &mut state.iface, &mut state.device, &mut state.sockets);

    if state.device.frames_emitted.swap(false, Ordering::Relaxed) {
        shared.rx_wake.wake();
        emitted = true;
    }

    IterationResult {
        frames_emitted: emitted,
        new_connections: new_conn_count,
    }
}

/// Main smoltcp poll loop. Runs on a dedicated OS thread.
///
/// Processes guest frames with pre-inspection, drives smoltcp's TCP/IP
/// stack, and sleeps via `poll(2)` between events.
///
/// # Phases per iteration
///
/// 1. **Drain guest frames** — pop from `tx_ring`, classify, pre-inspect.
/// 2. **smoltcp egress + maintenance** — transmit queued packets, run timers.
/// 3. **Service connections** — relay data between smoltcp sockets and proxy
///    tasks (proxy spawning added by Phase 7).
/// 4. **Sleep** — `poll(2)` on `tx_wake` + `proxy_wake` pipes with smoltcp's
///    requested timeout.
pub fn smoltcp_poll_loop(
    shared: Arc<SharedState>,
    config: PollLoopConfig,
    tokio_handle: tokio::runtime::Handle,
) {
    let mut device = SmoltcpDevice::new(shared.clone(), config.mtu);
    let iface = create_interface(&mut device, &config);

    let mut state = PollLoopState {
        device,
        iface,
        sockets: SocketSet::new(vec![]),
        conn_tracker: ConnectionTracker::new(None),
        dns_interceptor: DnsInterceptor::new(
            &mut SocketSet::new(vec![]),
            shared.clone(),
            &tokio_handle,
        ),
        udp_relay: UdpRelay::new(
            shared.clone(),
            config.gateway_mac,
            config.guest_mac,
            tokio_handle.clone(),
        ),
        last_cleanup: std::time::Instant::now(),
    };

    // Re-create dns_interceptor with the state's socket set.
    state.dns_interceptor = DnsInterceptor::new(&mut state.sockets, shared.clone(), &tokio_handle);

    let mut poll_fds = [
        libc::pollfd {
            fd: shared.tx_wake.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: shared.proxy_wake.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
    ];

    loop {
        poll_iteration(&mut state, &shared, &config, &tokio_handle);

        let now = smoltcp_now();
        let timeout_ms = state
            .iface
            .poll_delay(now, &state.sockets)
            .map(|d| d.total_millis().min(i32::MAX as u64) as i32)
            .unwrap_or(100);

        // SAFETY: poll_fds is a valid array of pollfd structs with valid fds.
        unsafe {
            libc::poll(
                poll_fds.as_mut_ptr(),
                poll_fds.len() as libc::nfds_t,
                timeout_ms,
            );
        }

        if poll_fds[0].revents & libc::POLLIN != 0 {
            shared.tx_wake.drain();
        }
        if poll_fds[1].revents & libc::POLLIN != 0 {
            shared.proxy_wake.drain();
        }
    }
}

//--------------------------------------------------------------------------------------------------
// Helpers
//--------------------------------------------------------------------------------------------------

/// Drain all pending egress frames from the smoltcp interface.
fn drain_egress(
    now: Instant,
    iface: &mut Interface,
    device: &mut SmoltcpDevice,
    sockets: &mut SocketSet<'_>,
) {
    loop {
        let result = iface.poll_egress(now, device, sockets);
        if matches!(result, smoltcp::iface::PollResult::None) {
            break;
        }
    }
}

fn classify_ipv4(payload: &[u8]) -> FrameAction {
    let Ok(ipv4) = Ipv4Packet::new_checked(payload) else {
        return FrameAction::Passthrough;
    };
    let src_ip = std::net::IpAddr::V4(ipv4.src_addr().into());
    let dst_ip = std::net::IpAddr::V4(ipv4.dst_addr().into());
    classify_transport(ipv4.next_header(), src_ip, dst_ip, ipv4.payload())
}

fn classify_transport(
    protocol: IpProtocol,
    src_ip: std::net::IpAddr,
    dst_ip: std::net::IpAddr,
    transport_payload: &[u8],
) -> FrameAction {
    match protocol {
        IpProtocol::Tcp => {
            let Ok(tcp) = TcpPacket::new_checked(transport_payload) else {
                return FrameAction::Passthrough;
            };
            if tcp.syn() && !tcp.ack() {
                FrameAction::TcpSyn {
                    src: SocketAddr::new(src_ip, tcp.src_port()),
                    dst: SocketAddr::new(dst_ip, tcp.dst_port()),
                }
            } else {
                FrameAction::Passthrough
            }
        }
        IpProtocol::Udp => {
            let Ok(udp) = UdpPacket::new_checked(transport_payload) else {
                return FrameAction::Passthrough;
            };
            if udp.dst_port() == 53 {
                FrameAction::Dns
            } else {
                FrameAction::UdpRelay {
                    src: SocketAddr::new(src_ip, udp.src_port()),
                    dst: SocketAddr::new(dst_ip, udp.dst_port()),
                }
            }
        }
        _ => FrameAction::Passthrough,
    }
}

/// Get the current time as a smoltcp [`Instant`] using a monotonic clock.
fn smoltcp_now() -> Instant {
    static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let epoch = EPOCH.get_or_init(std::time::Instant::now);
    let elapsed = epoch.elapsed();
    Instant::from_millis(elapsed.as_millis() as i64)
}

//--------------------------------------------------------------------------------------------------
// Tests
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal Ethernet + IPv4 + TCP SYN frame.
    fn build_tcp_syn_frame(
        src_ip: [u8; 4],
        dst_ip: [u8; 4],
        src_port: u16,
        dst_port: u16,
    ) -> Vec<u8> {
        let mut frame = vec![0u8; 14 + 20 + 20]; // eth + ipv4 + tcp

        frame[12] = 0x08; // EtherType: IPv4
        frame[13] = 0x00;

        let ip = &mut frame[14..34];
        ip[0] = 0x45; // Version + IHL
        let total_len = 40u16; // 20 (IP) + 20 (TCP)
        ip[2..4].copy_from_slice(&total_len.to_be_bytes());
        ip[6] = 0x40; // Don't Fragment
        ip[8] = 64; // TTL
        ip[9] = 6; // Protocol: TCP
        ip[12..16].copy_from_slice(&src_ip);
        ip[16..20].copy_from_slice(&dst_ip);

        let tcp = &mut frame[34..54];
        tcp[0..2].copy_from_slice(&src_port.to_be_bytes());
        tcp[2..4].copy_from_slice(&dst_port.to_be_bytes());
        tcp[12] = 0x50; // Data offset: 5 words
        tcp[13] = 0x02; // SYN flag

        frame
    }

    /// Build a minimal Ethernet + IPv4 + UDP frame.
    fn build_udp_frame(src_ip: [u8; 4], dst_ip: [u8; 4], src_port: u16, dst_port: u16) -> Vec<u8> {
        let mut frame = vec![0u8; 14 + 20 + 8]; // eth + ipv4 + udp

        frame[12] = 0x08;
        frame[13] = 0x00;

        let ip = &mut frame[14..34];
        ip[0] = 0x45;
        let total_len = 28u16; // 20 (IP) + 8 (UDP)
        ip[2..4].copy_from_slice(&total_len.to_be_bytes());
        ip[8] = 64;
        ip[9] = 17; // Protocol: UDP
        ip[12..16].copy_from_slice(&src_ip);
        ip[16..20].copy_from_slice(&dst_ip);

        let udp = &mut frame[34..42];
        udp[0..2].copy_from_slice(&src_port.to_be_bytes());
        udp[2..4].copy_from_slice(&dst_port.to_be_bytes());
        let udp_len = 8u16;
        udp[4..6].copy_from_slice(&udp_len.to_be_bytes());

        frame
    }

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
        tokio_handle: &tokio::runtime::Handle,
    ) -> PollLoopState {
        let mut device = SmoltcpDevice::new(shared.clone(), config.mtu);
        let iface = create_interface(&mut device, config);
        let mut sockets = SocketSet::new(vec![]);
        let dns_interceptor = DnsInterceptor::new(&mut sockets, shared.clone(), tokio_handle);
        let udp_relay = UdpRelay::new(
            shared.clone(),
            config.gateway_mac,
            config.guest_mac,
            tokio_handle.clone(),
        );

        PollLoopState {
            device,
            iface,
            sockets,
            conn_tracker: ConnectionTracker::new(None),
            dns_interceptor,
            udp_relay,
            last_cleanup: std::time::Instant::now(),
        }
    }

    // -- classify_frame tests (existing) --

    #[test]
    fn classify_tcp_syn() {
        let frame = build_tcp_syn_frame([10, 0, 0, 2], [93, 184, 216, 34], 54321, 443);
        match classify_frame(&frame) {
            FrameAction::TcpSyn { src, dst } => {
                assert_eq!(
                    src,
                    SocketAddr::new(Ipv4Addr::new(10, 0, 0, 2).into(), 54321)
                );
                assert_eq!(
                    dst,
                    SocketAddr::new(Ipv4Addr::new(93, 184, 216, 34).into(), 443)
                );
            }
            _ => panic!("expected TcpSyn"),
        }
    }

    #[test]
    fn classify_tcp_ack_is_passthrough() {
        let mut frame = build_tcp_syn_frame([10, 0, 0, 2], [93, 184, 216, 34], 54321, 443);
        frame[34 + 13] = 0x10; // ACK flag
        assert!(matches!(classify_frame(&frame), FrameAction::Passthrough));
    }

    #[test]
    fn classify_udp_dns() {
        let frame = build_udp_frame([10, 0, 0, 2], [10, 0, 0, 1], 12345, 53);
        assert!(matches!(classify_frame(&frame), FrameAction::Dns));
    }

    #[test]
    fn classify_udp_non_dns() {
        let frame = build_udp_frame([10, 0, 0, 2], [8, 8, 8, 8], 12345, 443);
        match classify_frame(&frame) {
            FrameAction::UdpRelay { src, dst } => {
                assert_eq!(src.port(), 12345);
                assert_eq!(dst.port(), 443);
            }
            _ => panic!("expected UdpRelay"),
        }
    }

    #[test]
    fn classify_arp_is_passthrough() {
        let mut frame = vec![0u8; 42];
        frame[12] = 0x08;
        frame[13] = 0x06; // EtherType: ARP
        assert!(matches!(classify_frame(&frame), FrameAction::Passthrough));
    }

    #[test]
    fn classify_garbage_is_passthrough() {
        assert!(matches!(classify_frame(&[]), FrameAction::Passthrough));
        assert!(matches!(classify_frame(&[0; 5]), FrameAction::Passthrough));
    }

    // -- poll_iteration tests --

    #[tokio::test]
    async fn poll_iteration_empty_is_noop() {
        let shared = Arc::new(SharedState::new(64));
        let config = test_config();
        let handle = tokio::runtime::Handle::current();
        let mut state = build_state(&shared, &config, &handle);

        let result = poll_iteration(&mut state, &shared, &config, &handle);
        assert!(!result.frames_emitted);
        assert_eq!(result.new_connections, 0);
    }

    #[tokio::test]
    async fn poll_iteration_tcp_syn_creates_connection() {
        let shared = Arc::new(SharedState::new(64));
        let config = test_config();
        let handle = tokio::runtime::Handle::current();
        let mut state = build_state(&shared, &config, &handle);

        let syn = build_tcp_syn_frame([10, 0, 2, 2], [93, 184, 216, 34], 54321, 80);
        shared.tx_ring.push(syn).unwrap();

        let src: SocketAddr = "10.0.2.2:54321".parse().unwrap();
        let dst: SocketAddr = "93.184.216.34:80".parse().unwrap();

        assert!(!state.conn_tracker.has_socket_for(&src, &dst));
        let _result = poll_iteration(&mut state, &shared, &config, &handle);
        assert!(
            state.conn_tracker.has_socket_for(&src, &dst),
            "SYN should create a tracked connection"
        );
    }

    #[tokio::test]
    async fn poll_iteration_multiple_frames() {
        let shared = Arc::new(SharedState::new(64));
        let config = test_config();
        let handle = tokio::runtime::Handle::current();
        let mut state = build_state(&shared, &config, &handle);

        let syn1 = build_tcp_syn_frame([10, 0, 2, 2], [93, 184, 216, 34], 10001, 80);
        let syn2 = build_tcp_syn_frame([10, 0, 2, 2], [93, 184, 216, 34], 10002, 443);
        shared.tx_ring.push(syn1).unwrap();
        shared.tx_ring.push(syn2).unwrap();

        let _result = poll_iteration(&mut state, &shared, &config, &handle);

        let src1: SocketAddr = "10.0.2.2:10001".parse().unwrap();
        let dst1: SocketAddr = "93.184.216.34:80".parse().unwrap();
        let src2: SocketAddr = "10.0.2.2:10002".parse().unwrap();
        let dst2: SocketAddr = "93.184.216.34:443".parse().unwrap();

        assert!(state.conn_tracker.has_socket_for(&src1, &dst1));
        assert!(state.conn_tracker.has_socket_for(&src2, &dst2));
    }

    #[tokio::test]
    async fn poll_iteration_dns_frame_processed() {
        let shared = Arc::new(SharedState::new(64));
        let config = test_config();
        let handle = tokio::runtime::Handle::current();
        let mut state = build_state(&shared, &config, &handle);

        let dns_frame = build_udp_frame([10, 0, 2, 2], [10, 0, 2, 1], 12345, 53);
        shared.tx_ring.push(dns_frame).unwrap();

        let result = poll_iteration(&mut state, &shared, &config, &handle);
        assert_eq!(
            result.new_connections, 0,
            "DNS should not create TCP connections"
        );
    }
}
