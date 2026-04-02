//! DNS query interception and resolution.
//!
//! The [`DnsInterceptor`] bridges the smoltcp UDP socket (bound to port 53)
//! and the host DNS resolvers via hickory-resolver. Queries are read from
//! the socket, forwarded to a background tokio task for resolution, and
//! responses are sent back through the socket on the next poll iteration.

use std::sync::Arc;

use bytes::Bytes;
use smoltcp::iface::SocketSet;
use smoltcp::socket::udp;
use smoltcp::storage::PacketMetadata;
use smoltcp::wire::{IpEndpoint, IpListenEndpoint};
use tokio::sync::mpsc;

use crate::shared::SharedState;

const DNS_PORT: u16 = 53;
const DNS_MAX_SIZE: usize = 4096;
const DNS_SOCKET_PACKET_SLOTS: usize = 16;
const CHANNEL_CAPACITY: usize = 64;

/// DNS query/response interceptor.
///
/// Owns the smoltcp UDP socket handle and channels to the async resolver
/// task. The poll loop calls [`process()`] each iteration to shuttle
/// queries and responses between smoltcp and hickory-resolver.
///
/// [`process()`]: DnsInterceptor::process
pub struct DnsInterceptor {
    socket_handle: smoltcp::iface::SocketHandle,
    query_tx: mpsc::Sender<DnsQuery>,
    response_rx: mpsc::Receiver<DnsResponse>,
}

struct DnsQuery {
    data: Bytes,
    source: IpEndpoint,
}

struct DnsResponse {
    data: Bytes,
    dest: IpEndpoint,
}

impl DnsInterceptor {
    /// Create the DNS interceptor.
    ///
    /// Binds a smoltcp UDP socket to port 53, creates the channel pair, and
    /// spawns the background resolver task.
    pub fn new(
        sockets: &mut SocketSet<'_>,
        shared: Arc<SharedState>,
        tokio_handle: &tokio::runtime::Handle,
    ) -> Self {
        let rx_meta = vec![PacketMetadata::EMPTY; DNS_SOCKET_PACKET_SLOTS];
        let rx_payload = vec![0u8; DNS_MAX_SIZE * DNS_SOCKET_PACKET_SLOTS];
        let tx_meta = vec![PacketMetadata::EMPTY; DNS_SOCKET_PACKET_SLOTS];
        let tx_payload = vec![0u8; DNS_MAX_SIZE * DNS_SOCKET_PACKET_SLOTS];

        let mut socket = udp::Socket::new(
            udp::PacketBuffer::new(rx_meta, rx_payload),
            udp::PacketBuffer::new(tx_meta, tx_payload),
        );
        socket
            .bind(IpListenEndpoint {
                addr: None,
                port: DNS_PORT,
            })
            .expect("failed to bind DNS socket to port 53");

        let socket_handle = sockets.add(socket);

        let (query_tx, query_rx) = mpsc::channel(CHANNEL_CAPACITY);
        let (response_tx, response_rx) = mpsc::channel(CHANNEL_CAPACITY);

        tokio_handle.spawn(dns_resolver_task(query_rx, response_tx, shared));

        Self {
            socket_handle,
            query_tx,
            response_rx,
        }
    }

    /// Process DNS queries and responses.
    ///
    /// Called by the poll loop each iteration:
    /// 1. Reads queries from the smoltcp socket and sends to resolver task.
    /// 2. Reads responses from the resolver and writes to smoltcp socket.
    pub fn process(&mut self, sockets: &mut SocketSet<'_>) {
        let socket = sockets.get_mut::<udp::Socket>(self.socket_handle);

        let mut buf = [0u8; DNS_MAX_SIZE];
        while socket.can_recv() {
            match socket.recv_slice(&mut buf) {
                Ok((n, meta)) => {
                    let query = DnsQuery {
                        data: Bytes::copy_from_slice(&buf[..n]),
                        source: meta.endpoint,
                    };
                    if self.query_tx.try_send(query).is_err() {
                        tracing::debug!("DNS query channel full, dropping query");
                    }
                }
                Err(_) => break,
            }
        }

        while socket.can_send() {
            match self.response_rx.try_recv() {
                Ok(response) => {
                    let _ = socket.send_slice(&response.data, response.dest);
                }
                Err(_) => break,
            }
        }
    }
}

async fn dns_resolver_task(
    mut query_rx: mpsc::Receiver<DnsQuery>,
    response_tx: mpsc::Sender<DnsResponse>,
    shared: Arc<SharedState>,
) {
    let resolver = match hickory_resolver::Resolver::builder_tokio().map(|b| b.build()) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(error = %e, "failed to create DNS resolver");
            return;
        }
    };

    while let Some(query) = query_rx.recv().await {
        let response_tx = response_tx.clone();
        let shared = shared.clone();
        let resolver = resolver.clone();

        tokio::spawn(async move {
            if let Some(response_data) = resolve_query(&query.data, &resolver).await {
                let response = DnsResponse {
                    data: response_data,
                    dest: query.source,
                };
                if response_tx.send(response).await.is_ok() {
                    shared.proxy_wake.wake();
                }
            }
        });
    }
}

async fn resolve_query(
    raw_query: &[u8],
    resolver: &hickory_resolver::TokioResolver,
) -> Option<Bytes> {
    use hickory_proto::op::Message;
    use hickory_proto::serialize::binary::BinDecodable;

    let query_msg = Message::from_bytes(raw_query).ok()?;
    let query_id = query_msg.id();

    let question = query_msg.queries().first()?;
    let record_type = question.query_type();

    let lookup = resolver
        .lookup(question.name().clone(), record_type)
        .await
        .ok()?;

    let mut response_msg = Message::new();
    response_msg.set_id(query_id);
    response_msg.set_message_type(hickory_proto::op::MessageType::Response);
    response_msg.set_op_code(query_msg.op_code());
    response_msg.set_response_code(hickory_proto::op::ResponseCode::NoError);
    response_msg.set_recursion_desired(query_msg.recursion_desired());
    response_msg.set_recursion_available(true);
    response_msg.add_query(question.clone());

    let answers: Vec<_> = lookup.records().to_vec();
    response_msg.insert_answers(answers);

    use hickory_proto::serialize::binary::BinEncodable;
    let response_bytes = response_msg.to_bytes().ok()?;

    Some(Bytes::from(response_bytes))
}
