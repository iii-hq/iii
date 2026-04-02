//! smoltcp interface setup, frame classification, and poll loop.
//!
//! This module contains the core networking event loop that runs on a
//! dedicated OS thread. It bridges guest ethernet frames (via
//! [`SmoltcpDevice`]) to smoltcp's TCP/IP stack and services connections
//! through tokio proxy tasks.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::Ordering;
use std::sync::Arc;

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
            .push(IpCidr::new(
                IpAddress::from(config.gateway_ipv4),
                30,
            ))
            .expect("failed to add gateway IPv4 address");
    });

    iface
        .routes_mut()
        .add_default_ipv4_route(config.gateway_ipv4.into())
        .expect("failed to add default IPv4 route");

    iface.set_any_ip(true);

    iface
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
    let mut iface = create_interface(&mut device, &config);
    let mut sockets = SocketSet::new(vec![]);
    let mut conn_tracker = ConnectionTracker::new(None);
    let mut dns_interceptor = DnsInterceptor::new(
        &mut sockets,
        shared.clone(),
        &tokio_handle,
    );
    let mut udp_relay = UdpRelay::new(
        shared.clone(),
        config.gateway_mac,
        config.guest_mac,
        tokio_handle.clone(),
    );

    let mut last_cleanup = std::time::Instant::now();

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
        let now = smoltcp_now();

        // Phase 1: Drain all guest frames with pre-inspection.
        while let Some(frame) = device.stage_next_frame() {
            match classify_frame(frame) {
                FrameAction::TcpSyn { src, dst } => {
                    if !conn_tracker.has_socket_for(&src, &dst) {
                        conn_tracker.create_tcp_socket(src, dst, &mut sockets);
                    }
                    iface.poll_ingress_single(now, &mut device, &mut sockets);
                }
                FrameAction::UdpRelay { src, dst } => {
                    udp_relay.relay_outbound(frame, src, dst);
                    device.drop_staged_frame();
                }
                FrameAction::Dns | FrameAction::Passthrough => {
                    iface.poll_ingress_single(now, &mut device, &mut sockets);
                }
            }
        }

        // Phase 2: Egress + maintenance.
        loop {
            let result = iface.poll_egress(now, &mut device, &mut sockets);
            if matches!(result, smoltcp::iface::PollResult::None) {
                break;
            }
        }
        iface.poll_maintenance(now);

        if device.frames_emitted.swap(false, Ordering::Relaxed) {
            shared.rx_wake.wake();
        }

        // Phase 3: Service connections.
        conn_tracker.relay_data(&mut sockets);
        dns_interceptor.process(&mut sockets);

        let new_conns = conn_tracker.take_new_connections(&mut sockets);
        for conn in new_conns {
            proxy::spawn_tcp_proxy(
                &tokio_handle,
                conn.dst,
                conn.from_smoltcp,
                conn.to_smoltcp,
                shared.clone(),
                config.gateway_ipv4,
            );
        }

        if last_cleanup.elapsed() >= std::time::Duration::from_secs(1) {
            conn_tracker.cleanup_closed(&mut sockets);
            udp_relay.cleanup_expired();
            last_cleanup = std::time::Instant::now();
        }

        // Phase 4: Flush + sleep.
        loop {
            let result = iface.poll_egress(now, &mut device, &mut sockets);
            if matches!(result, smoltcp::iface::PollResult::None) {
                break;
            }
        }

        if device.frames_emitted.swap(false, Ordering::Relaxed) {
            shared.rx_wake.wake();
        }

        let timeout_ms = iface
            .poll_delay(now, &sockets)
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
    fn build_udp_frame(
        src_ip: [u8; 4],
        dst_ip: [u8; 4],
        src_port: u16,
        dst_port: u16,
    ) -> Vec<u8> {
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
}
