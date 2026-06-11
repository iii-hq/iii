//! Non-DNS UDP relay: handles UDP traffic outside smoltcp (IPv4 only).
//!
//! smoltcp has no wildcard port binding, so non-DNS UDP is intercepted at
//! the device level, relayed through host UDP sockets via tokio, and
//! responses are injected back into `rx_ring` as constructed ethernet frames.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use smoltcp::wire::{
    EthernetAddress, EthernetFrame, EthernetProtocol, EthernetRepr, IpAddress, IpProtocol,
    Ipv4Packet, UdpPacket,
};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use crate::shared::SharedState;

const SESSION_TIMEOUT: Duration = Duration::from_secs(60);
const OUTBOUND_CHANNEL_CAPACITY: usize = 64;
const RECV_BUF_SIZE: usize = 4096;
const ETH_HDR_LEN: usize = 14;
const IPV4_HDR_LEN: usize = 20;
const UDP_HDR_LEN: usize = 8;
/// Largest UDP payload an IPv4 datagram can carry: the IP total-length
/// field is 16-bit, minus the IP and UDP headers. Anything bigger would
/// silently truncate in the `as u16` header writes below.
const MAX_UDP_PAYLOAD: usize = u16::MAX as usize - IPV4_HDR_LEN - UDP_HDR_LEN;

/// Relays non-DNS UDP traffic between the guest and the real network.
///
/// Each unique `(guest_src, guest_dst)` pair gets a host-side UDP socket
/// and a tokio relay task. The poll loop calls [`relay_outbound()`] to
/// send guest datagrams; response frames are injected directly into
/// `rx_ring`.
///
/// [`relay_outbound()`]: UdpRelay::relay_outbound
pub struct UdpRelay {
    shared: Arc<SharedState>,
    sessions: HashMap<(SocketAddr, SocketAddr), UdpSession>,
    gateway_mac: EthernetAddress,
    guest_mac: EthernetAddress,
    tokio_handle: tokio::runtime::Handle,
}

struct UdpSession {
    outbound_tx: mpsc::Sender<Bytes>,
    last_active: Instant,
}

impl UdpRelay {
    pub fn new(
        shared: Arc<SharedState>,
        gateway_mac: [u8; 6],
        guest_mac: [u8; 6],
        tokio_handle: tokio::runtime::Handle,
    ) -> Self {
        Self {
            shared,
            sessions: HashMap::new(),
            gateway_mac: EthernetAddress(gateway_mac),
            guest_mac: EthernetAddress(guest_mac),
            tokio_handle,
        }
    }

    /// Relay an outbound UDP datagram from the guest.
    pub fn relay_outbound(&mut self, frame: &[u8], src: SocketAddr, dst: SocketAddr) {
        let Some(payload) = extract_udp_payload(frame) else {
            return;
        };

        let key = (src, dst);

        if self
            .sessions
            .get(&key)
            .is_none_or(|s| s.last_active.elapsed() > SESSION_TIMEOUT)
        {
            self.sessions.remove(&key);
            if let Some(session) = self.create_session(src, dst) {
                self.sessions.insert(key, session);
            } else {
                return;
            }
        }

        if let Some(session) = self.sessions.get_mut(&key) {
            session.last_active = Instant::now();
            let _ = session
                .outbound_tx
                .try_send(Bytes::copy_from_slice(payload));
        }
    }

    /// Remove expired sessions.
    pub fn cleanup_expired(&mut self) {
        self.sessions
            .retain(|_, session| session.last_active.elapsed() <= SESSION_TIMEOUT);
    }
}

impl UdpRelay {
    fn create_session(&self, guest_src: SocketAddr, guest_dst: SocketAddr) -> Option<UdpSession> {
        let (outbound_tx, outbound_rx) = mpsc::channel(OUTBOUND_CHANNEL_CAPACITY);

        let shared = self.shared.clone();
        let gateway_mac = self.gateway_mac;
        let guest_mac = self.guest_mac;

        self.tokio_handle.spawn(async move {
            if let Err(e) = udp_relay_task(
                outbound_rx,
                guest_src,
                guest_dst,
                shared,
                gateway_mac,
                guest_mac,
            )
            .await
            {
                tracing::debug!(
                    guest_src = %guest_src,
                    guest_dst = %guest_dst,
                    error = %e,
                    "UDP relay task ended",
                );
            }
        });

        Some(UdpSession {
            outbound_tx,
            last_active: Instant::now(),
        })
    }
}

async fn udp_relay_task(
    mut outbound_rx: mpsc::Receiver<Bytes>,
    guest_src: SocketAddr,
    guest_dst: SocketAddr,
    shared: Arc<SharedState>,
    gateway_mac: EthernetAddress,
    guest_mac: EthernetAddress,
) -> std::io::Result<()> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0u16)).await?;
    socket.connect(guest_dst).await?;

    let mut recv_buf = vec![0u8; RECV_BUF_SIZE];

    loop {
        tokio::select! {
            data = outbound_rx.recv() => {
                match data {
                    Some(payload) => {
                        let _ = socket.send(&payload).await;
                    }
                    None => break,
                }
            }

            result = socket.recv(&mut recv_buf) => {
                match result {
                    Ok(n) => {
                        let frame = construct_udp_response_v4(
                            guest_dst,
                            guest_src,
                            &recv_buf[..n],
                            gateway_mac,
                            guest_mac,
                        );
                        // Empty = construction refused (non-IPv4 addrs or
                        // oversized payload); don't inject a zero-length
                        // frame into the guest's rx ring.
                        if !frame.is_empty() {
                            let _ = shared.rx_ring.push(frame);
                            shared.rx_wake.wake();
                        }
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, "UDP relay recv failed");
                        break;
                    }
                }
            }

            () = tokio::time::sleep(SESSION_TIMEOUT) => {
                break;
            }
        }
    }

    Ok(())
}

fn construct_udp_response_v4(
    src: SocketAddr,
    dst: SocketAddr,
    payload: &[u8],
    gateway_mac: EthernetAddress,
    guest_mac: EthernetAddress,
) -> Vec<u8> {
    let src_ip = match src.ip() {
        std::net::IpAddr::V4(v4) => v4,
        _ => return Vec::new(),
    };
    let dst_ip = match dst.ip() {
        std::net::IpAddr::V4(v4) => v4,
        _ => return Vec::new(),
    };
    if payload.len() > MAX_UDP_PAYLOAD {
        return Vec::new();
    }

    let udp_len = UDP_HDR_LEN + payload.len();
    let ip_total_len = IPV4_HDR_LEN + udp_len;
    let frame_len = ETH_HDR_LEN + ip_total_len;
    let mut buf = vec![0u8; frame_len];

    let eth_repr = EthernetRepr {
        src_addr: gateway_mac,
        dst_addr: guest_mac,
        ethertype: EthernetProtocol::Ipv4,
    };
    let mut eth_frame = EthernetFrame::new_unchecked(&mut buf);
    eth_repr.emit(&mut eth_frame);

    let ip_buf = &mut buf[ETH_HDR_LEN..];
    let mut ip_pkt = Ipv4Packet::new_unchecked(ip_buf);
    ip_pkt.set_version(4);
    ip_pkt.set_header_len(20);
    ip_pkt.set_total_len(ip_total_len as u16);
    ip_pkt.clear_flags();
    ip_pkt.set_dont_frag(true);
    ip_pkt.set_hop_limit(64);
    ip_pkt.set_next_header(IpProtocol::Udp);
    ip_pkt.set_src_addr(src_ip);
    ip_pkt.set_dst_addr(dst_ip);
    ip_pkt.fill_checksum();

    let udp_buf = &mut buf[ETH_HDR_LEN + IPV4_HDR_LEN..];
    let mut udp_pkt = UdpPacket::new_unchecked(udp_buf);
    udp_pkt.set_src_port(src.port());
    udp_pkt.set_dst_port(dst.port());
    udp_pkt.set_len(udp_len as u16);
    udp_pkt.payload_mut()[..payload.len()].copy_from_slice(payload);
    // Real checksum, not the "no checksum" zero sentinel: strict guest
    // stacks drop zero-checksum UDP, and the pseudo-header sum catches
    // corruption between the relay and the guest. Must run AFTER the
    // payload write — the sum covers it.
    udp_pkt.fill_checksum(&IpAddress::from(src_ip), &IpAddress::from(dst_ip));

    buf
}

fn extract_udp_payload(frame: &[u8]) -> Option<&[u8]> {
    let eth = EthernetFrame::new_checked(frame).ok()?;
    match eth.ethertype() {
        EthernetProtocol::Ipv4 => {
            let ipv4 = Ipv4Packet::new_checked(eth.payload()).ok()?;
            let udp = UdpPacket::new_checked(ipv4.payload()).ok()?;
            Some(udp.payload())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construct_v4_response_has_correct_structure() {
        let payload = b"hello";
        let src: SocketAddr = (Ipv4Addr::new(8, 8, 8, 8), 53).into();
        let dst: SocketAddr = (Ipv4Addr::new(100, 96, 0, 2), 12345).into();
        let frame = construct_udp_response_v4(
            src,
            dst,
            payload,
            EthernetAddress([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]),
            EthernetAddress([0x02, 0x00, 0x00, 0x00, 0x00, 0x02]),
        );

        assert_eq!(frame.len(), ETH_HDR_LEN + IPV4_HDR_LEN + UDP_HDR_LEN + 5);

        let eth = EthernetFrame::new_checked(&frame).unwrap();
        assert_eq!(eth.ethertype(), EthernetProtocol::Ipv4);
        assert_eq!(
            eth.dst_addr(),
            EthernetAddress([0x02, 0x00, 0x00, 0x00, 0x00, 0x02])
        );

        let ipv4 = Ipv4Packet::new_checked(eth.payload()).unwrap();
        assert_eq!(Ipv4Addr::from(ipv4.src_addr()), Ipv4Addr::new(8, 8, 8, 8));
        assert_eq!(
            Ipv4Addr::from(ipv4.dst_addr()),
            Ipv4Addr::new(100, 96, 0, 2)
        );
        assert_eq!(ipv4.next_header(), IpProtocol::Udp);

        let udp = UdpPacket::new_checked(ipv4.payload()).unwrap();
        assert_eq!(udp.src_port(), 53);
        assert_eq!(udp.dst_port(), 12345);
        assert_eq!(udp.payload(), b"hello");
    }

    #[test]
    fn extract_payload_from_v4_udp_frame() {
        let src: SocketAddr = (Ipv4Addr::new(1, 2, 3, 4), 80).into();
        let dst: SocketAddr = (Ipv4Addr::new(10, 0, 0, 2), 54321).into();
        let frame = construct_udp_response_v4(
            src,
            dst,
            b"test data",
            EthernetAddress([0; 6]),
            EthernetAddress([0; 6]),
        );
        let payload = extract_udp_payload(&frame).unwrap();
        assert_eq!(payload, b"test data");
    }

    /// Injected response frames must carry a REAL UDP checksum, not the
    /// "no checksum" zero sentinel — guests with strict UDP validation
    /// drop zero-checksum datagrams, and a real checksum catches
    /// corruption between the relay and the guest stack.
    #[test]
    fn construct_v4_response_fills_udp_checksum() {
        let src_ip = Ipv4Addr::new(8, 8, 8, 8);
        let dst_ip = Ipv4Addr::new(100, 96, 0, 2);
        let src: SocketAddr = (src_ip, 53).into();
        let dst: SocketAddr = (dst_ip, 12345).into();
        let frame = construct_udp_response_v4(
            src,
            dst,
            b"checksummed payload",
            EthernetAddress([0x02, 0, 0, 0, 0, 1]),
            EthernetAddress([0x02, 0, 0, 0, 0, 2]),
        );

        let eth = EthernetFrame::new_checked(&frame).unwrap();
        let ipv4 = Ipv4Packet::new_checked(eth.payload()).unwrap();
        assert!(ipv4.verify_checksum(), "IPv4 header checksum invalid");

        let udp = UdpPacket::new_checked(ipv4.payload()).unwrap();
        assert_ne!(udp.checksum(), 0, "UDP checksum left as the zero sentinel");
        assert!(
            udp.verify_checksum(&IpAddress::from(src_ip), &IpAddress::from(dst_ip)),
            "UDP checksum does not verify against the pseudo-header"
        );
    }

    /// A payload that would overflow the 16-bit IP/UDP length fields must
    /// be rejected, not silently truncated into a corrupt header.
    #[test]
    fn construct_v4_response_rejects_oversized_payload() {
        let src: SocketAddr = (Ipv4Addr::new(8, 8, 8, 8), 53).into();
        let dst: SocketAddr = (Ipv4Addr::new(100, 96, 0, 2), 12345).into();
        let oversized = vec![0u8; MAX_UDP_PAYLOAD + 1];
        let frame = construct_udp_response_v4(
            src,
            dst,
            &oversized,
            EthernetAddress([0; 6]),
            EthernetAddress([0; 6]),
        );
        assert!(frame.is_empty(), "oversized payload must yield no frame");

        // The largest legal payload still produces a well-formed frame.
        let max = vec![0u8; MAX_UDP_PAYLOAD];
        let frame = construct_udp_response_v4(
            src,
            dst,
            &max,
            EthernetAddress([0; 6]),
            EthernetAddress([0; 6]),
        );
        assert_eq!(frame.len(), ETH_HDR_LEN + IPV4_HDR_LEN + UDP_HDR_LEN + max.len());
        assert_eq!(extract_udp_payload(&frame).unwrap(), &max[..]);
    }
}
