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
    let (mut server_rx, mut server_tx) = stream.into_split();

    let mut server_buf = vec![0u8; SERVER_READ_BUF_SIZE];

    loop {
        tokio::select! {
            data = from_smoltcp.recv() => {
                match data {
                    Some(bytes) => {
                        if let Err(e) = server_tx.write_all(&bytes).await {
                            tracing::debug!(dst = %dst, error = %e, "write to server failed");
                            break;
                        }
                    }
                    None => break,
                }
            }

            result = server_rx.read(&mut server_buf) => {
                match result {
                    Ok(0) => break,
                    Ok(n) => {
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
