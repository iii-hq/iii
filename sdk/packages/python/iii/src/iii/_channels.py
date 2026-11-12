"""The WebSocket ends of a channel (internal).

:class:`~iii.InputStream` and :class:`~iii.OutputStream` are built on these:
one socket per end at ``<engine>/ws/channels/<id>?key=<access_key>&dir=read|write``.
Binary frames carry bytes and text frames carry messages. A clean close
(1000, or no code) ends the stream; any other close aborts it, and the engine
relays an abort to the reader as close 1011.
"""

from __future__ import annotations

import asyncio
import logging
import os
import weakref
from urllib.parse import quote

import websockets
from websockets.asyncio.client import ClientConnection
from websockets.frames import CloseCode

from .errors import StreamAbortedError
from .iii_types import StreamChannelRef

log = logging.getLogger("iii.channels")

MAX_FRAME_SIZE = 64 * 1024
# A stalled reader stops the engine reading the writer socket; websockets' 10 s default would then drop the
# queued clean close, which the engine reads as an abort. Same bound as the engine's channel TTL.
WRITER_CLOSE_TIMEOUT_S = 5 * 60
# No keepalive pings on channel sockets (the other SDKs send none): behind a slow consumer the pong queues after
# megabytes of data, and websockets would fail a healthy stream with a ping timeout.
CHANNEL_PING_INTERVAL: float | None = None
#: The close code an aborting writer sends. The engine relays every close other
#: than 1000 (or no code) to the reader as an abort (close 1011).
ABORT_CLOSE_CODE = 1011
#: A WebSocket close reason holds at most 123 bytes.
MAX_CLOSE_REASON_BYTES = 123
#: How long an abort waits for its close frame to go out before it drops the socket.
ABORT_CLOSE_TIMEOUT_S = 5.0


def resolve_headers(explicit: dict[str, str] | None = None) -> dict[str, str] | None:
    """Handshake headers for an engine connection: the caller's, plus
    Authorization: Bearer $III_TOKEN when the variable is set and not blank.

    An Authorization header the caller passed wins (names match
    case-insensitively).
    """
    token = os.environ.get("III_TOKEN", "").strip()
    if not token or any(name.lower() == "authorization" for name in explicit or {}):
        return explicit
    return {**(explicit or {}), "Authorization": f"Bearer {token}"}


def build_channel_url(
    engine_ws_base: str,
    channel_id: str,
    access_key: str,
    direction: str,
) -> str:
    # Keep a query already on the engine URL (e.g. ?token=...); the channel's own come after it.
    base, _, query = engine_ws_base.partition("?")
    query = "&".join(part for part in (query, f"key={quote(access_key)}", f"dir={direction}") if part)
    return f"{base.rstrip('/')}/ws/channels/{channel_id}?{query}"


def truncate_reason(reason: str) -> str:
    """``reason`` cut to the WebSocket close-reason limit, on a UTF-8 character boundary."""
    encoded = reason.encode("utf-8", errors="replace")
    if len(encoded) <= MAX_CLOSE_REASON_BYTES:
        return reason
    return encoded[:MAX_CLOSE_REASON_BYTES].decode("utf-8", errors="ignore")


def _abort_transport(loop: asyncio.AbstractEventLoop, transport: asyncio.Transport) -> None:
    """Drop a socket with no close frame, from any thread: the engine reads that as an abort."""
    if not loop.is_closed():
        loop.call_soon_threadsafe(transport.abort)


class ChannelWriter:
    """The write end of a channel: binary frames, text frames, a clean close or an abort.

    Every method runs on the loop that opened the socket (the caller makes sure of it).
    """

    def __init__(self, engine_ws_base: str, ref: StreamChannelRef, headers: dict[str, str] | None = None) -> None:
        self._url = build_channel_url(engine_ws_base, ref.channel_id, ref.access_key, "write")
        self._headers = resolve_headers(headers)
        self._ws: ClientConnection | None = None
        self._connected = False
        # One operation at a time, in call order: a close queued after a write waits for all of its frames.
        self._lock = asyncio.Lock()
        self._abort_cause: BaseException | None = None
        self._aborted = False

    def _abort(self, cause: BaseException) -> None:
        """A send that did not finish (an error, or a cancelled task): the stream is incomplete,
        so drop the socket with no close frame and never close it cleanly."""
        self._abort_cause = self._abort_cause or cause
        self._connected = False
        if self._ws is not None:
            self._ws.transport.abort()

    async def _ensure_connected(self) -> ClientConnection:
        """Open the socket on first use. The caller holds ``_lock``."""
        if self._abort_cause is not None:
            raise ConnectionError("channel writer failed: an earlier write did not finish") from self._abort_cause
        if self._aborted:
            raise ConnectionError("channel writer aborted")
        if self._ws is not None and self._connected:
            return self._ws
        if self._ws is not None:
            raise ConnectionError("channel writer closed")
        self._ws = await websockets.connect(
            self._url,
            additional_headers=self._headers,
            close_timeout=WRITER_CLOSE_TIMEOUT_S,
            ping_interval=CHANNEL_PING_INTERVAL,
        )
        self._connected = True
        # A live loop keeps the socket open forever, so a writer dropped without close() must end it itself.
        weakref.finalize(self, _abort_transport, asyncio.get_running_loop(), self._ws.transport)
        return self._ws

    async def write(self, data: bytes) -> None:
        """Binary frames of at most :data:`MAX_FRAME_SIZE` bytes; returns once the socket took them."""
        try:
            async with self._lock:
                ws = await self._ensure_connected()
                if len(data) <= MAX_FRAME_SIZE:
                    await ws.send(data)
                else:
                    offset = 0
                    while offset < len(data):
                        await ws.send(data[offset : offset + MAX_FRAME_SIZE])
                        offset += MAX_FRAME_SIZE
        except BaseException as e:
            self._abort(e)
            raise

    async def send_text(self, msg: str) -> None:
        """One text frame."""
        try:
            async with self._lock:
                ws = await self._ensure_connected()
                await ws.send(msg)
        except BaseException as e:
            self._abort(e)
            raise

    async def close(self) -> None:
        """End the stream with a clean close frame. A writer that never opened its
        socket opens it first, so the reader sees an empty stream rather than
        waiting for one. A writer whose write failed is not closed cleanly: its
        stream already lost data."""
        try:
            # Delay the close frame slightly to allow the TCP stack to flush
            # all buffered send() data. Without this, the close frame can arrive
            # at the engine before all data frames, causing data truncation.
            await asyncio.sleep(0.01)
        finally:
            # Also when the delay is cancelled at loop teardown: every write finished (an unfinished
            # one aborted the socket), so end cleanly.
            async with self._lock:
                if self._ws is None and self._abort_cause is None and not self._aborted:
                    await self._ensure_connected()
                if self._ws is not None and self._connected:
                    await self._ws.close()
                    self._connected = False

    async def abort(self, reason: str) -> None:
        """End the stream as failed: a close frame with :data:`ABORT_CLOSE_CODE` and
        ``reason``. A writer that never opened its socket opens it first, so the
        reader always learns of the failure. Does not wait for a write in flight."""
        if self._aborted:
            return
        self._aborted = True
        ws = self._ws
        if ws is None and self._abort_cause is None:
            try:
                ws = self._ws = await websockets.connect(
                    self._url,
                    additional_headers=self._headers,
                    close_timeout=ABORT_CLOSE_TIMEOUT_S,
                    ping_interval=CHANNEL_PING_INTERVAL,
                )
                self._connected = True
            except Exception as e:
                log.debug("Could not open the channel to abort it: %s", e)
                return
        if ws is None or not self._connected:
            return
        self._connected = False
        try:
            await asyncio.wait_for(ws.close(ABORT_CLOSE_CODE, truncate_reason(reason)), ABORT_CLOSE_TIMEOUT_S)
        except Exception:
            ws.transport.abort()


class ChannelReader:
    """The read end of a channel: frames in order, ``None`` at a clean end."""

    def __init__(self, engine_ws_base: str, ref: StreamChannelRef, headers: dict[str, str] | None = None) -> None:
        self._url = build_channel_url(engine_ws_base, ref.channel_id, ref.access_key, "read")
        self._headers = resolve_headers(headers)
        self._ws: ClientConnection | None = None
        self._connected = False
        self._lock = asyncio.Lock()
        self._closed_locally = False

    async def _ensure_connected(self) -> ClientConnection:
        if self._ws is not None:
            return self._ws
        async with self._lock:
            if self._ws is not None:
                return self._ws
            self._ws = await websockets.connect(
                self._url, additional_headers=self._headers, ping_interval=CHANNEL_PING_INTERVAL
            )
            self._connected = True
            return self._ws

    @property
    def opened(self) -> bool:
        return self._ws is not None

    async def recv(self) -> bytes | str | None:
        """The next frame: ``bytes`` (binary) or ``str`` (text); ``None`` once the
        writer closed the stream cleanly (close 1000 or no code), or after a
        local :meth:`close`. Any other end raises :class:`StreamAbortedError`
        once the frames received before it have been returned."""
        if self._closed_locally and self._ws is None:
            return None
        ws = await self._ensure_connected()
        try:
            return await ws.recv()
        except websockets.ConnectionClosed as e:
            # websockets also counts 1001 as clean; a channel does not. No code (1005) is the engine's clean end.
            if self._closed_locally or (
                e.rcvd is not None and e.rcvd.code in (CloseCode.NORMAL_CLOSURE, CloseCode.NO_STATUS_RCVD)
            ):
                return None
            code, reason = (int(e.rcvd.code), e.rcvd.reason) if e.rcvd else (int(CloseCode.ABNORMAL_CLOSURE), str(e))
            raise StreamAbortedError(reason, close_code=code) from e

    async def read_all(self) -> bytes:
        """Every binary frame up to the end, joined (text frames are skipped)."""
        chunks: list[bytes] = []
        while (frame := await self.recv()) is not None:
            if isinstance(frame, bytes):
                chunks.append(frame)
        return b"".join(chunks)

    async def close(self, *, open_first: bool = False) -> None:
        """Stop reading. The engine then closes the writer. ``open_first`` opens a
        socket that never opened, so the writer learns the reader left."""
        self._closed_locally = True
        if self._ws is None and open_first:
            try:
                await self._ensure_connected()
            except Exception as e:
                log.debug("Could not open the channel to cancel it: %s", e)
                return
        if self._ws is not None and self._connected:
            # The engine does not answer a reader's close, so the end that follows is not an abort.
            self._connected = False
            await self._ws.close()
