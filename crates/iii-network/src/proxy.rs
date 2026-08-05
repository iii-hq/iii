//! Bidirectional TCP proxy: smoltcp socket <-> channels <-> tokio socket.
//!
//! Each outbound guest TCP connection gets a proxy task that opens a real
//! TCP connection to the destination via tokio and relays data between the
//! channel pair (connected to the smoltcp socket in the poll loop) and the
//! real server.
//!
//! Connections to the gateway IP are rewritten to 127.0.0.1 — the gateway
//! represents the host from the guest's perspective (like QEMU's 10.0.2.2).

use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use bytes::Bytes;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::shared::SharedState;

const SERVER_READ_BUF_SIZE: usize = 16384;

/// Spawn a TCP proxy task for a newly established connection.
///
/// Connects to `dst` via tokio, then bidirectionally relays data between
/// the smoltcp socket (via channels) and the real server. Wakes the poll
/// thread via `shared.proxy_wake` whenever data is sent toward the guest.
///
/// If `dst` targets `gateway_ipv4`, the connection is redirected to
/// `127.0.0.1` (the host loopback) since the gateway IP is virtual.
pub fn spawn_tcp_proxy(
    handle: &tokio::runtime::Handle,
    dst: SocketAddr,
    from_smoltcp: mpsc::Receiver<Bytes>,
    to_smoltcp: mpsc::Sender<Bytes>,
    shared: Arc<SharedState>,
    gateway_ipv4: Ipv4Addr,
) {
    handle.spawn(async move {
        if let Err(e) = tcp_proxy_task(dst, from_smoltcp, to_smoltcp, shared, gateway_ipv4).await {
            tracing::debug!(dst = %dst, error = %e, "TCP proxy task ended");
        }
    });
}

/// Rewrite the destination address: if the guest targeted the gateway IP,
/// connect to localhost instead (the gateway is the host from the guest's
/// perspective).
fn resolve_host_dst(dst: SocketAddr, gateway_ipv4: Ipv4Addr) -> SocketAddr {
    match dst.ip() {
        std::net::IpAddr::V4(ip) if ip == gateway_ipv4 => {
            SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), dst.port())
        }
        _ => dst,
    }
}

async fn tcp_proxy_task(
    dst: SocketAddr,
    mut from_smoltcp: mpsc::Receiver<Bytes>,
    to_smoltcp: mpsc::Sender<Bytes>,
    shared: Arc<SharedState>,
    gateway_ipv4: Ipv4Addr,
) -> io::Result<()> {
    let host_dst = resolve_host_dst(dst, gateway_ipv4);
    let stream = TcpStream::connect(host_dst).await?;
    // The other half of the Nagle fix in `conn::create_tcp_socket`. This relay
    // has TWO delegate sockets — the smoltcp one facing the guest and this
    // kernel one facing the destination — and leaving Nagle on either
    // reintroduces the same stall, just on the other leg: small relayed
    // writes held awaiting the peer's delayed ACK (~40ms against a Linux
    // peer). Only loopback destinations were exonerated by measurement,
    // because loopback ACKs immediately.
    if let Err(e) = stream.set_nodelay(true) {
        tracing::debug!(%dst, error = %e, "set_nodelay failed; small writes may stall");
    }
    tracing::debug!(%dst, %host_dst, "proxy connected");
    let (mut server_rx, mut server_tx) = stream.into_split();

    let mut server_buf = vec![0u8; SERVER_READ_BUF_SIZE];
    let mut guest_open = true;

    loop {
        tokio::select! {
            data = from_smoltcp.recv(), if guest_open => {
                match data {
                    Some(bytes) => {
                        // Guest payload moved: refresh the idle-reaper beacon.
                        // This channel only ever carries application data
                        // (conn.rs relays recv'd bytes, never bare frames).
                        shared.note_activity();
                        if let Err(e) = server_tx.write_all(&bytes).await {
                            tracing::debug!(dst = %dst, error = %e, "write to server failed");
                            break;
                        }
                    }
                    None => {
                        // Guest sent FIN: half-close toward the server but
                        // keep relaying its remaining response bytes back.
                        guest_open = false;
                        let _ = server_tx.shutdown().await;
                    }
                }
            }

            result = server_rx.read(&mut server_buf) => {
                match result {
                    Ok(0) => break,
                    Ok(n) => {
                        // Response payload toward the guest counts too — a
                        // slow server reply keeps its sandbox alive.
                        shared.note_activity();
                        let data = Bytes::copy_from_slice(&server_buf[..n]);
                        if to_smoltcp.send(data).await.is_err() {
                            break;
                        }
                        shared.proxy_wake.wake();
                    }
                    Err(e) => {
                        tracing::debug!(dst = %dst, error = %e, "read from server failed");
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::ActivityStamp;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::{Duration, SystemTime};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn aged_beacon(name: &str) -> (std::path::PathBuf, SystemTime) {
        let path = std::env::temp_dir().join(format!(
            "iii-net-activity-proxy-{}-{name}",
            std::process::id()
        ));
        let old = SystemTime::now() - Duration::from_secs(600);
        (path, old)
    }

    fn age(path: &std::path::Path, to: SystemTime) {
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(to)
            .unwrap();
    }

    fn assert_refreshed(path: &std::path::Path, old: SystemTime, dir: &str) {
        let mtime = std::fs::metadata(path).unwrap().modified().unwrap();
        assert!(
            mtime > old + Duration::from_secs(300),
            "{dir} payload must refresh the activity beacon (mtime {mtime:?} vs aged {old:?})"
        );
        let _ = std::fs::remove_file(path);
    }

    /// The regression the beacon closes: a resident guest process serving
    /// request/response traffic over this proxy was reaped as idle because
    /// nothing daemon-visible recorded the traffic. Guest→server payload
    /// must refresh the beacon.
    #[tokio::test]
    async fn guest_to_server_payload_refreshes_activity_beacon() {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        let (path, old) = aged_beacon("g2s");
        let shared = Arc::new(SharedState::with_activity(
            64,
            Some(ActivityStamp::create(&path).unwrap()),
        ));
        // Age BEFORE any stamp so the once-per-second throttle gate is
        // untouched when the payload flows.
        age(&path, old);

        let (to_task_tx, to_task_rx) = mpsc::channel(8);
        let (from_task_tx, _from_task_rx) = mpsc::channel(8);
        let gateway = Ipv4Addr::new(10, 0, 2, 2);
        let _task = tokio::spawn(tcp_proxy_task(
            addr,
            to_task_rx,
            from_task_tx,
            shared.clone(),
            gateway,
        ));
        let (mut server_conn, _) = listener.accept().await.unwrap();

        to_task_tx.send(Bytes::from_static(b"ping")).await.unwrap();
        let mut buf = [0u8; 4];
        server_conn.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping");

        assert_refreshed(&path, old, "guest->server");
    }

    /// ...and the response leg: a server reply toward the guest is equally
    /// "the sandbox is doing work".
    #[tokio::test]
    async fn server_to_guest_payload_refreshes_activity_beacon() {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        let (path, old) = aged_beacon("s2g");
        let shared = Arc::new(SharedState::with_activity(
            64,
            Some(ActivityStamp::create(&path).unwrap()),
        ));
        age(&path, old);

        let (_to_task_tx, to_task_rx) = mpsc::channel::<Bytes>(8);
        let (from_task_tx, mut from_task_rx) = mpsc::channel(8);
        let gateway = Ipv4Addr::new(10, 0, 2, 2);
        let _task = tokio::spawn(tcp_proxy_task(
            addr,
            to_task_rx,
            from_task_tx,
            shared.clone(),
            gateway,
        ));
        let (mut server_conn, _) = listener.accept().await.unwrap();

        server_conn.write_all(b"pong").await.unwrap();
        let got = from_task_rx.recv().await.expect("relayed response");
        assert_eq!(&got[..], b"pong");

        assert_refreshed(&path, old, "server->guest");
    }

    #[test]
    fn resolve_host_dst_rewrites_gateway_to_localhost() {
        let gateway = Ipv4Addr::new(10, 0, 2, 2);
        let dst = SocketAddr::new(IpAddr::V4(gateway), 8080);
        let result = resolve_host_dst(dst, gateway);
        assert_eq!(
            result,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080)
        );
    }

    #[test]
    fn resolve_host_dst_preserves_non_gateway_ipv4() {
        let gateway = Ipv4Addr::new(10, 0, 2, 2);
        let dst = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)), 443);
        let result = resolve_host_dst(dst, gateway);
        assert_eq!(result, dst);
    }

    #[test]
    fn resolve_host_dst_preserves_ipv6() {
        let gateway = Ipv4Addr::new(10, 0, 2, 2);
        let dst = SocketAddr::new(IpAddr::V6(std::net::Ipv6Addr::LOCALHOST), 80);
        let result = resolve_host_dst(dst, gateway);
        assert_eq!(result, dst);
    }

    #[test]
    fn resolve_host_dst_preserves_port_on_rewrite() {
        let gateway = Ipv4Addr::new(10, 0, 2, 2);
        let dst = SocketAddr::new(IpAddr::V4(gateway), 49134);
        let result = resolve_host_dst(dst, gateway);
        assert_eq!(result.port(), 49134);
        assert_eq!(result.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
    }

    #[test]
    fn resolve_host_dst_different_gateway() {
        let gateway = Ipv4Addr::new(192, 168, 1, 1);
        let dst = SocketAddr::new(IpAddr::V4(gateway), 3000);
        let result = resolve_host_dst(dst, gateway);
        assert_eq!(
            result,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 3000)
        );
    }
}
