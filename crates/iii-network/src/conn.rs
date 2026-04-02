//! Connection tracker: manages smoltcp TCP sockets for the poll loop.
//!
//! Creates sockets on SYN detection, tracks connection lifecycle, relays data
//! between smoltcp sockets and proxy task channels, and cleans up closed
//! connections.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;

use bytes::Bytes;
use smoltcp::iface::{SocketHandle, SocketSet};
use smoltcp::socket::tcp;
use smoltcp::wire::IpListenEndpoint;
use tokio::sync::mpsc;

const TCP_RX_BUF_SIZE: usize = 65536;
const TCP_TX_BUF_SIZE: usize = 65536;
const DEFAULT_MAX_CONNECTIONS: usize = 256;
const CHANNEL_CAPACITY: usize = 32;
const RELAY_BUF_SIZE: usize = 16384;
const DEFERRED_CLOSE_LIMIT: u16 = 64;

/// Tracks TCP connections between guest and proxy tasks.
///
/// Each guest TCP connection maps to a smoltcp socket and a pair of channels
/// connecting it to a tokio proxy task. The tracker handles:
///
/// - **Socket creation** — on SYN detection, before smoltcp processes the frame.
/// - **Data relay** — shuttles bytes between smoltcp sockets and channels.
/// - **Lifecycle detection** — identifies newly-established connections for
///   proxy spawning.
/// - **Cleanup** — removes closed sockets from the socket set.
pub struct ConnectionTracker {
    connections: HashMap<SocketHandle, Connection>,
    connection_keys: HashSet<(SocketAddr, SocketAddr)>,
    max_connections: usize,
}

struct Connection {
    src: SocketAddr,
    dst: SocketAddr,
    to_proxy: mpsc::Sender<Bytes>,
    from_proxy: mpsc::Receiver<Bytes>,
    proxy_channels: Option<ProxyChannels>,
    proxy_spawned: bool,
    write_buf: Option<(Bytes, usize)>,
    close_attempts: u16,
}

struct ProxyChannels {
    from_smoltcp: mpsc::Receiver<Bytes>,
    to_smoltcp: mpsc::Sender<Bytes>,
}

/// Information for spawning a proxy task for a newly established connection.
///
/// Returned by [`ConnectionTracker::take_new_connections()`]. The poll loop
/// passes this to the proxy task spawner.
pub struct NewConnection {
    pub dst: SocketAddr,
    pub from_smoltcp: mpsc::Receiver<Bytes>,
    pub to_smoltcp: mpsc::Sender<Bytes>,
}

impl ConnectionTracker {
    pub fn new(max_connections: Option<usize>) -> Self {
        Self {
            connections: HashMap::new(),
            connection_keys: HashSet::new(),
            max_connections: max_connections.unwrap_or(DEFAULT_MAX_CONNECTIONS),
        }
    }

    /// O(1) duplicate-SYN detection via HashSet lookup.
    pub fn has_socket_for(&self, src: &SocketAddr, dst: &SocketAddr) -> bool {
        self.connection_keys.contains(&(*src, *dst))
    }

    /// Create a smoltcp TCP socket for an incoming SYN and register it.
    ///
    /// The socket is put into LISTEN state on the destination IP + port so
    /// smoltcp will complete the three-way handshake when it processes the
    /// SYN frame. Returns `false` if at `max_connections` limit.
    pub fn create_tcp_socket(
        &mut self,
        src: SocketAddr,
        dst: SocketAddr,
        sockets: &mut SocketSet<'_>,
    ) -> bool {
        if self.connections.len() >= self.max_connections {
            return false;
        }

        let rx_buf = tcp::SocketBuffer::new(vec![0u8; TCP_RX_BUF_SIZE]);
        let tx_buf = tcp::SocketBuffer::new(vec![0u8; TCP_TX_BUF_SIZE]);
        let mut socket = tcp::Socket::new(rx_buf, tx_buf);

        let listen_addr: smoltcp::wire::IpAddress = match dst.ip() {
            std::net::IpAddr::V4(v4) => v4.into(),
            std::net::IpAddr::V6(_) => return false,
        };
        let listen_endpoint = IpListenEndpoint {
            addr: Some(listen_addr),
            port: dst.port(),
        };
        if socket.listen(listen_endpoint).is_err() {
            return false;
        }

        let handle = sockets.add(socket);

        let (to_proxy_tx, to_proxy_rx) = mpsc::channel(CHANNEL_CAPACITY);
        let (from_proxy_tx, from_proxy_rx) = mpsc::channel(CHANNEL_CAPACITY);

        self.connection_keys.insert((src, dst));
        self.connections.insert(
            handle,
            Connection {
                src,
                dst,
                to_proxy: to_proxy_tx,
                from_proxy: from_proxy_rx,
                proxy_channels: Some(ProxyChannels {
                    from_smoltcp: to_proxy_rx,
                    to_smoltcp: from_proxy_tx,
                }),
                proxy_spawned: false,
                write_buf: None,
                close_attempts: 0,
            },
        );

        true
    }

    /// Relay data between smoltcp sockets and proxy task channels.
    ///
    /// For each connection with a spawned proxy:
    /// - Reads data from the smoltcp socket and sends it to the proxy channel.
    /// - Receives data from the proxy channel and writes it to the smoltcp socket.
    pub fn relay_data(&mut self, sockets: &mut SocketSet<'_>) {
        let mut relay_buf = [0u8; RELAY_BUF_SIZE];

        for (&handle, conn) in &mut self.connections {
            if !conn.proxy_spawned {
                continue;
            }

            let socket = sockets.get_mut::<tcp::Socket>(handle);

            if conn.to_proxy.is_closed() {
                write_proxy_data(socket, conn);
                if conn.write_buf.is_none() {
                    socket.close();
                } else {
                    conn.close_attempts += 1;
                    if conn.close_attempts >= DEFERRED_CLOSE_LIMIT {
                        socket.abort();
                    }
                }
                continue;
            }

            while socket.can_recv() {
                match socket.recv_slice(&mut relay_buf) {
                    Ok(n) if n > 0 => {
                        let data = Bytes::copy_from_slice(&relay_buf[..n]);
                        if conn.to_proxy.try_send(data).is_err() {
                            break;
                        }
                    }
                    _ => break,
                }
            }

            write_proxy_data(socket, conn);
        }
    }

    /// Collect newly-established connections that need proxy tasks.
    pub fn take_new_connections(&mut self, sockets: &mut SocketSet<'_>) -> Vec<NewConnection> {
        let mut new = Vec::new();

        for (&handle, conn) in &mut self.connections {
            if conn.proxy_spawned {
                continue;
            }

            let socket = sockets.get::<tcp::Socket>(handle);
            if socket.state() == tcp::State::Established {
                conn.proxy_spawned = true;

                if let Some(channels) = conn.proxy_channels.take() {
                    new.push(NewConnection {
                        dst: conn.dst,
                        from_smoltcp: channels.from_smoltcp,
                        to_smoltcp: channels.to_smoltcp,
                    });
                }
            }
        }

        new
    }

    /// Remove closed connections and their sockets.
    ///
    /// Only removes sockets in the `Closed` state. Sockets in `TimeWait`
    /// are left for smoltcp to handle naturally (2*MSL timer).
    pub fn cleanup_closed(&mut self, sockets: &mut SocketSet<'_>) {
        let keys = &mut self.connection_keys;
        self.connections.retain(|&handle, conn| {
            let socket = sockets.get::<tcp::Socket>(handle);
            if matches!(socket.state(), tcp::State::Closed) {
                keys.remove(&(conn.src, conn.dst));
                sockets.remove(handle);
                false
            } else {
                true
            }
        });
    }
}

fn write_proxy_data(socket: &mut tcp::Socket<'_>, conn: &mut Connection) {
    if let Some((data, offset)) = &mut conn.write_buf {
        if socket.can_send() {
            match socket.send_slice(&data[*offset..]) {
                Ok(written) => {
                    *offset += written;
                    if *offset >= data.len() {
                        conn.write_buf = None;
                    }
                }
                Err(_) => return,
            }
        } else {
            return;
        }
    }

    while conn.write_buf.is_none() {
        match conn.from_proxy.try_recv() {
            Ok(data) => {
                if socket.can_send() {
                    match socket.send_slice(&data) {
                        Ok(written) if written < data.len() => {
                            conn.write_buf = Some((data, written));
                        }
                        Err(_) => {
                            conn.write_buf = Some((data, 0));
                        }
                        _ => {}
                    }
                } else {
                    conn.write_buf = Some((data, 0));
                }
            }
            Err(_) => break,
        }
    }
}
