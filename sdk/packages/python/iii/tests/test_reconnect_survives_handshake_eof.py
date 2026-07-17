"""MOT-3966 regression: the reconnect loop must survive handshake-level failures.

First-boot conditions: the engine autostarts bundle workers before its worker
listener is accepting, and the VM's userspace network stack (iii-network)
accepts the guest's TCP connection before dialing the host — so "nothing
listening yet" surfaces to the client as an EOF between TCP connect and the
WS 101. The websockets library raises that as ``InvalidMessage`` (an
``InvalidHandshake``, NOT a ``ConnectionError``/``OSError``). The SDK's old
narrow ``except`` let it escape ``_do_connect``, silently killing the connect
task with zero retries: the worker stayed a zombie for the VM's lifetime.
"""

import base64
import hashlib
import socket
import threading

from iii.iii import III
from iii.iii_constants import InitOptions
from iii_helpers.observability import ReconnectionConfig

WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"


def _minimal_ws_server(listener: socket.socket, eof_first_n: int) -> None:
    """Accept loop: the first ``eof_first_n`` connections are closed without
    sending a byte (EOF during the client's handshake — the engine-boot race
    as seen through iii-network's accept-then-dial). Later connections get a
    real RFC 6455 101 reply and are held open."""
    held = []
    accepted = 0
    while True:
        try:
            conn, _ = listener.accept()
        except OSError:
            return  # listener closed: test over
        accepted += 1
        if accepted <= eof_first_n:
            conn.close()
            continue

        data = b""
        try:
            while b"\r\n\r\n" not in data:
                chunk = conn.recv(4096)
                if not chunk:
                    break
                data += chunk
            key = ""
            for line in data.decode("latin1").split("\r\n"):
                if line.lower().startswith("sec-websocket-key:"):
                    key = line.split(":", 1)[1].strip()
            accept = base64.b64encode(
                hashlib.sha1((key + WS_GUID).encode()).digest()
            ).decode()
            conn.sendall(
                b"HTTP/1.1 101 Switching Protocols\r\n"
                b"Upgrade: websocket\r\n"
                b"Connection: Upgrade\r\n"
                b"Sec-WebSocket-Accept: " + accept.encode() + b"\r\n\r\n"
            )
        except OSError:
            conn.close()
            continue
        held.append(conn)  # keep open; the client streams registrations


def test_reconnect_survives_handshake_eof(monkeypatch) -> None:
    monkeypatch.setattr(
        "iii_helpers.observability.telemetry.init_otel", lambda **kwargs: None
    )
    monkeypatch.setattr(
        "iii_helpers.observability.telemetry.attach_event_loop", lambda loop: None
    )

    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen(8)
    port = listener.getsockname()[1]
    server = threading.Thread(
        target=_minimal_ws_server, args=(listener, 1), daemon=True
    )
    server.start()

    client = III(
        f"ws://127.0.0.1:{port}",
        InitOptions(
            reconnection_config=ReconnectionConfig(
                initial_delay_ms=100,
                max_delay_ms=200,
                jitter_factor=0.0,
            ),
        ),
    )
    try:
        # Pre-fix: the first connection's EOF raised InvalidMessage out of
        # _do_connect, the connect task died silently, no retry ever ran,
        # and this wait timed out with the client stuck "connecting".
        client._connected_event.wait(timeout=10)
        assert client._connection_state == "connected", (
            f"client never recovered from the handshake EOF "
            f"(state={client._connection_state!r}; reconnect loop died)"
        )
    finally:
        client.shutdown()
        listener.close()
