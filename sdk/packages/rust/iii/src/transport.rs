//! WebSocket transport shared by the worker connection and the channel ends.

use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async_with_config,
    tungstenite::{
        Error, client::IntoClientRequest, handshake::client::Response, protocol::WebSocketConfig,
    },
};

use crate::protocol::{ENGINE_CAPABILITIES_HEADER, ENGINE_CAPABILITY_TCP_NODELAY};

/// A client WebSocket to the engine, over plain TCP or TLS.
pub(crate) type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// What opening a [`WsStream`] returns.
pub(crate) type WsConnectResult = Result<(WsStream, Response), Error>;

/// Opens a WebSocket to the engine and negotiates `TCP_NODELAY` with it (see
/// [`negotiate_nodelay`]). The socket connects with Nagle on.
pub(crate) async fn connect<R>(request: R, config: Option<WebSocketConfig>) -> WsConnectResult
where
    R: IntoClientRequest + Unpin,
{
    let (stream, response) = connect_async_with_config(request, config, false).await?;
    let tcp_nodelay = negotiate_nodelay(&stream, &response);
    tracing::debug!(tcp_nodelay, "connected to the engine");
    Ok((stream, response))
}

/// Disables Nagle on this end only when the engine advertised
/// [`ENGINE_CAPABILITY_TCP_NODELAY`], i.e. its own end already has
/// `TCP_NODELAY` (MOT-5377). Invocations and results are small frames, and
/// with Nagle on a second small write waits for the peer's delayed ACK
/// (~40 ms on Linux). With Nagle on the engine's end alone, a worker that
/// answers immediately leaves the engine's next small write waiting for that
/// ACK, which measured slower than keeping Nagle on both ends. Returns whether
/// `TCP_NODELAY` is now set.
fn negotiate_nodelay(stream: &WsStream, response: &Response) -> bool {
    if !engine_has_capability(response, ENGINE_CAPABILITY_TCP_NODELAY) {
        return false;
    }
    let tcp = match stream.get_ref() {
        MaybeTlsStream::Plain(tcp) => tcp,
        MaybeTlsStream::Rustls(tls) => tls.get_ref().0,
        _ => {
            tracing::debug!("unsupported TLS backend; keeping Nagle on");
            return false;
        }
    };
    match tcp.set_nodelay(true) {
        Ok(()) => true,
        Err(err) => {
            tracing::debug!(error = %err, "set_nodelay failed; small writes may stall");
            false
        }
    }
}

/// Whether the engine listed `capability` in [`ENGINE_CAPABILITIES_HEADER`].
fn engine_has_capability(response: &Response, capability: &str) -> bool {
    response
        .headers()
        .get_all(ENGINE_CAPABILITIES_HEADER)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|token| token.trim().eq_ignore_ascii_case(capability))
}

/// A one-connection WebSocket server standing in for the engine, for tests.
#[cfg(test)]
pub(crate) mod test_engine {
    use std::net::SocketAddr;

    use tokio::{net::TcpStream, task::JoinHandle};
    use tokio_tungstenite::{
        MaybeTlsStream, WebSocketStream,
        tungstenite::handshake::server::{Request, Response},
    };

    use crate::protocol::{ENGINE_CAPABILITIES_HEADER, ENGINE_CAPABILITY_TCP_NODELAY};

    use super::WsStream;

    /// Accepts one WebSocket and, when `advertise` is set, lists
    /// `tcp-nodelay` in the upgrade response like a patched engine.
    #[expect(
        clippy::result_large_err,
        reason = "tungstenite fixes the handshake callback's error type"
    )]
    pub(crate) async fn spawn(
        advertise: bool,
    ) -> (SocketAddr, JoinHandle<WebSocketStream<TcpStream>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            tokio_tungstenite::accept_hdr_async(
                socket,
                move |_: &Request, mut response: Response| {
                    if advertise {
                        response.headers_mut().insert(
                            ENGINE_CAPABILITIES_HEADER,
                            ENGINE_CAPABILITY_TCP_NODELAY.parse().unwrap(),
                        );
                    }
                    Ok(response)
                },
            )
            .await
            .unwrap()
        });
        (address, server)
    }

    /// Whether the client end of `stream` has `TCP_NODELAY`.
    pub(crate) fn nodelay(stream: &WsStream) -> bool {
        let MaybeTlsStream::Plain(tcp) = stream.get_ref() else {
            panic!("expected a plain TCP connection");
        };
        tcp.nodelay().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use tokio_tungstenite::tungstenite::http;

    use super::*;

    fn response_with(values: &[&str]) -> Response {
        let mut builder = http::Response::builder();
        for value in values {
            builder = builder.header(ENGINE_CAPABILITIES_HEADER, *value);
        }
        builder.body(None).unwrap()
    }

    #[test]
    fn capability_found_in_a_single_token() {
        assert!(engine_has_capability(
            &response_with(&["tcp-nodelay"]),
            ENGINE_CAPABILITY_TCP_NODELAY
        ));
    }

    #[test]
    fn capability_found_in_a_list_regardless_of_case_and_spaces() {
        assert!(engine_has_capability(
            &response_with(&["other", "future, TCP-NoDelay "]),
            ENGINE_CAPABILITY_TCP_NODELAY
        ));
    }

    #[test]
    fn capability_absent_without_the_header() {
        assert!(!engine_has_capability(
            &response_with(&[]),
            ENGINE_CAPABILITY_TCP_NODELAY
        ));
    }

    #[test]
    fn similar_token_is_not_the_capability() {
        assert!(!engine_has_capability(
            &response_with(&["tcp-nodelay-v2"]),
            ENGINE_CAPABILITY_TCP_NODELAY
        ));
    }
}
