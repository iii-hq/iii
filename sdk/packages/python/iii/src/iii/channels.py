"""Streaming channel writer and reader backed by WebSocket connections."""

from __future__ import annotations

import asyncio
import logging
import os
import threading
import weakref
from concurrent.futures import Future
from dataclasses import dataclass
from enum import Enum
from typing import Any, AsyncIterator, Callable, Coroutine
from urllib.parse import quote

import websockets
from websockets.asyncio.client import ClientConnection
from websockets.frames import CloseCode

from .iii_types import StreamChannelRef

log = logging.getLogger("iii.channels")

MAX_FRAME_SIZE = 64 * 1024
# A stalled reader stops the engine reading the writer socket; websockets' 10 s default would then drop the
# queued clean close, which the engine reads as an abort. Same bound as the engine's channel TTL.
WRITER_CLOSE_TIMEOUT_S = 5 * 60
# No keepalive pings on channel sockets (the other SDKs send none): behind a slow consumer the pong queues after
# megabytes of data, and websockets would fail a healthy stream with a ping timeout.
CHANNEL_PING_INTERVAL: float | None = None

_sync_loop: asyncio.AbstractEventLoop | None = None
_sync_loop_lock = threading.Lock()


def _get_sync_loop() -> asyncio.AbstractEventLoop:
    """The loop that owns sockets opened from sync code: one live loop, so every later call finds its socket."""
    global _sync_loop
    with _sync_loop_lock:
        if _sync_loop is None:
            _sync_loop = asyncio.new_event_loop()
            threading.Thread(target=_sync_loop.run_forever, name="iii-channels", daemon=True).start()
        return _sync_loop


def _run_on(socket_loop: asyncio.AbstractEventLoop | None, coro: Coroutine[Any, Any, None]) -> None:
    """Run a sync call's coroutine where the writer's socket lives.

    A caller on a running loop cannot wait: the work is queued on that loop, or on
    the socket's loop when that is another live loop. A thread with no running loop
    waits for it on the socket's loop, or on the sync loop when no socket is open.
    """
    try:
        running: asyncio.AbstractEventLoop | None = asyncio.get_running_loop()
    except RuntimeError:
        running = None
    live = socket_loop if socket_loop is not None and socket_loop.is_running() else None
    if running is not None and live in (None, running):
        running.create_task(coro)
        return
    future = asyncio.run_coroutine_threadsafe(coro, live or _get_sync_loop())
    if running is None:
        future.result()
    else:
        future.add_done_callback(_log_failure)


def _log_failure(future: Future[None]) -> None:
    """Nothing waits for this call, so its error would be lost; a task on the caller's loop would log it."""
    error = None if future.cancelled() else future.exception()
    if error is not None:
        log.error("Channel writer call failed", exc_info=error)


def _abort_transport(loop: asyncio.AbstractEventLoop, transport: asyncio.Transport) -> None:
    """Drop a socket with no close frame, from any thread: the engine reads that as an abort."""
    if not loop.is_closed():
        loop.call_soon_threadsafe(transport.abort)


class ChannelDirection(str, Enum):
    """Direction of a streaming channel reference.

    Mirrors the Rust SDK's ``ChannelDirection`` enum. The string values
    (``"read"`` / ``"write"``) match the wire format used by
    :class:`StreamChannelRef.direction`.
    """

    READ = "read"
    WRITE = "write"


@dataclass
class ChannelItem:
    """A single frame transferred over a channel.

    Mirrors the Rust SDK's ``ChannelItem`` enum. Exactly one of ``text`` or
    ``binary`` is set; use :meth:`text_item` / :meth:`binary_item` to
    construct instances.
    """

    text: str | None = None
    binary: bytes | None = None

    def __post_init__(self) -> None:
        if (self.text is None) == (self.binary is None):
            raise ValueError("ChannelItem requires exactly one of `text` or `binary`")

    @classmethod
    def text_item(cls, value: str) -> "ChannelItem":
        return cls(text=value, binary=None)

    @classmethod
    def binary_item(cls, value: bytes) -> "ChannelItem":
        return cls(text=None, binary=value)

    @property
    def is_text(self) -> bool:
        return self.text is not None

    @property
    def is_binary(self) -> bool:
        return self.binary is not None


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


class WritableStream:
    """Writable stream interface backed by a ChannelWriter, matching Node.js Writable semantics."""

    def __init__(self, writer: ChannelWriter) -> None:
        self._writer = writer

    def write(self, data: bytes) -> None:
        """Binary write. Queued from a running loop; a sync caller waits for it."""
        _run_on(self._writer._loop, self._writer.write(data))

    def end(self, data: bytes | None = None) -> None:
        """Write optional final data and close the stream."""

        async def _write_and_close() -> None:
            if data is not None:
                await self._writer.write(data)
            await self._writer.close_async()

        _run_on(self._writer._loop, _write_and_close())


class ReadableStream:
    """Readable stream interface backed by a ChannelReader, matching Node.js Readable semantics."""

    def __init__(self, reader: ChannelReader) -> None:
        self._reader = reader

    async def __aiter__(self) -> AsyncIterator[bytes]:
        async for chunk in self._reader:
            yield chunk


class ChannelWriter:
    """WebSocket-backed writer for streaming binary data and text messages."""

    def __init__(
        self, engine_ws_base: str, ref: StreamChannelRef, headers: dict[str, str] | None = None
    ) -> None:
        self._url = build_channel_url(engine_ws_base, ref.channel_id, ref.access_key, "write")
        self._headers = resolve_headers(headers)
        self._ws: ClientConnection | None = None
        self._connected = False
        # One operation at a time, in call order: a close queued after a write waits for all of its frames.
        self._lock = asyncio.Lock()
        # The loop that owns the socket once connected: sync calls from another thread run there.
        self._loop: asyncio.AbstractEventLoop | None = None
        self._abort_cause: BaseException | None = None
        self.stream = WritableStream(self)

    def _abort(self, cause: BaseException) -> None:
        """A send that did not finish (an error, or asyncio.run cancelling it): the stream is incomplete,
        so drop the socket with no close frame and never close it cleanly."""
        self._abort_cause = self._abort_cause or cause
        self._connected = False
        if self._ws is not None:
            self._ws.transport.abort()

    async def _ensure_connected(self) -> ClientConnection:
        """Open the socket on first use. The caller holds ``_lock``."""
        if self._abort_cause is not None:
            raise ConnectionError("channel writer failed: an earlier write did not finish") from self._abort_cause
        if self._ws is not None and self._connected:
            return self._ws
        self._ws = await websockets.connect(
            self._url,
            additional_headers=self._headers,
            close_timeout=WRITER_CLOSE_TIMEOUT_S,
            ping_interval=CHANNEL_PING_INTERVAL,
        )
        self._connected = True
        self._loop = asyncio.get_running_loop()
        # A live loop keeps the socket open forever, so a writer dropped without close() must end it itself.
        weakref.finalize(self, _abort_transport, self._loop, self._ws.transport)
        return self._ws

    async def write(self, data: bytes) -> None:
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

    def send_message(self, msg: str) -> None:
        """Text message. Queued from a running loop; a sync caller waits for it."""
        _run_on(self._loop, self.send_message_async(msg))

    async def send_message_async(self, msg: str) -> None:
        try:
            async with self._lock:
                ws = await self._ensure_connected()
                await ws.send(msg)
        except BaseException as e:
            self._abort(e)
            raise

    def close(self) -> None:
        """End the stream with a clean close frame. Queued from a running loop; a sync caller waits for it.

        A writer that goes away without closing reaches the reader as an abort.
        """
        _run_on(self._loop, self.close_async())

    async def close_async(self) -> None:
        try:
            # Delay the close frame slightly to allow the TCP stack to flush
            # all buffered send() data. Without this, the close frame can arrive
            # at the engine before all data frames, causing data truncation.
            # Sync calls queued before this close start meanwhile and take the lock first.
            await asyncio.sleep(0.01)
        finally:
            # Also when asyncio.run cancels the delay at loop teardown: every write finished (an unfinished
            # one aborted the socket), so end cleanly.
            async with self._lock:
                if self._ws is not None and self._connected:
                    await self._ws.close()
                    self._connected = False


class ChannelReader:
    """WebSocket-backed reader for streaming binary data and text messages."""

    def __init__(
        self, engine_ws_base: str, ref: StreamChannelRef, headers: dict[str, str] | None = None
    ) -> None:
        self._url = build_channel_url(engine_ws_base, ref.channel_id, ref.access_key, "read")
        self._headers = resolve_headers(headers)
        self._ws: ClientConnection | None = None
        self._connected = False
        self._lock = asyncio.Lock()
        self._closed_locally = False
        self._message_callbacks: list[Callable[[str], Any]] = []
        self.stream = ReadableStream(self)

    async def _ensure_connected(self) -> ClientConnection:
        if self._ws is not None and self._connected:
            return self._ws
        async with self._lock:
            if self._ws is not None and self._connected:
                return self._ws
            self._ws = await websockets.connect(
                self._url, additional_headers=self._headers, ping_interval=CHANNEL_PING_INTERVAL
            )
            self._connected = True
            return self._ws

    def on_message(self, callback: Callable[[str], Any]) -> None:
        self._message_callbacks.append(callback)

    async def __aiter__(self) -> AsyncIterator[bytes]:
        """Async iterator that yields binary chunks and dispatches text messages to callbacks.

        It ends when the writer closed the stream cleanly (a close with code 1000
        or no code). Any other end raises :class:`ConnectionError` once the
        data received before it has been yielded.
        """
        ws = await self._ensure_connected()
        while True:
            try:
                message = await ws.recv()
            except websockets.ConnectionClosed as e:
                # websockets also counts 1001 as clean; a channel does not. No code (1005) is the engine's clean end.
                if self._closed_locally or (
                    e.rcvd is not None and e.rcvd.code in (CloseCode.NORMAL_CLOSURE, CloseCode.NO_STATUS_RCVD)
                ):
                    return
                code, reason = (e.rcvd.code, e.rcvd.reason) if e.rcvd else (CloseCode.ABNORMAL_CLOSURE, str(e))
                raise ConnectionError(f"channel aborted (close {int(code)}{': ' + reason if reason else ''})") from e
            if isinstance(message, bytes):
                yield message
            else:
                for cb in self._message_callbacks:
                    try:
                        cb(message)
                    except Exception:
                        log.exception("Error in channel message callback")

    async def read_all(self) -> bytes:
        """Read the entire stream into a single bytes object."""
        chunks: list[bytes] = []
        async for chunk in self:
            chunks.append(chunk)
        return b"".join(chunks)

    async def close_async(self) -> None:
        if self._ws is not None and self._connected:
            # The engine does not answer a reader's close, so the end that follows is not an abort.
            self._closed_locally = True
            await self._ws.close()
            self._connected = False
