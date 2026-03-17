"""Streaming channel writer and reader backed by WebSocket connections."""

from __future__ import annotations

import asyncio
import logging
from typing import Any, AsyncIterator, Callable
from urllib.parse import quote

import websockets
from websockets.asyncio.client import ClientConnection

from .iii_types import StreamChannelRef

log = logging.getLogger("iii.channels")

MAX_FRAME_SIZE = 64 * 1024


def build_channel_url(
    engine_ws_base: str,
    channel_id: str,
    access_key: str,
    direction: str,
) -> str:
    base = engine_ws_base.rstrip("/")
    return f"{base}/ws/channels/{channel_id}?key={quote(access_key)}&dir={direction}"


class WritableStream:
    """Writable stream interface backed by a ChannelWriter, matching Node.js Writable semantics."""

    def __init__(self, writer: ChannelWriter) -> None:
        self._writer = writer

    def write(self, data: bytes) -> None:
        """Fire-and-forget binary write. Queues an async write on the running loop."""
        try:
            loop = asyncio.get_running_loop()
            loop.create_task(self._writer.write(data))
        except RuntimeError:
            asyncio.run(self._writer.write(data))

    def end(self, data: bytes | None = None) -> None:
        """Write optional final data and close the stream."""
        try:
            loop = asyncio.get_running_loop()
            if data is not None:

                async def _write_and_close() -> None:
                    await self._writer.write(data)
                    await self._writer.close_async()

                loop.create_task(_write_and_close())
            else:
                loop.create_task(self._writer.close_async())
        except RuntimeError:
            if data is not None:

                async def _write_and_close() -> None:
                    await self._writer.write(data)
                    await self._writer.close_async()

                asyncio.run(_write_and_close())
            else:
                asyncio.run(self._writer.close_async())


class ReadableStream:
    """Readable stream interface backed by a ChannelReader, matching Node.js Readable semantics."""

    def __init__(self, reader: ChannelReader) -> None:
        self._reader = reader

    async def __aiter__(self) -> AsyncIterator[bytes]:
        async for chunk in self._reader:
            yield chunk


class ChannelWriter:
    """WebSocket-backed writer for streaming binary data and text messages."""

    def __init__(self, engine_ws_base: str, ref: StreamChannelRef) -> None:
        self._url = build_channel_url(engine_ws_base, ref.channel_id, ref.access_key, "write")
        self._ws: ClientConnection | None = None
        self._connected = False
        self._lock = asyncio.Lock()
        self.stream = WritableStream(self)

    async def _ensure_connected(self) -> ClientConnection:
        if self._ws is not None and self._connected:
            return self._ws
        async with self._lock:
            if self._ws is not None and self._connected:
                return self._ws
            self._ws = await websockets.connect(self._url)
            self._connected = True
            return self._ws

    async def write(self, data: bytes) -> None:
        ws = await self._ensure_connected()
        if len(data) <= MAX_FRAME_SIZE:
            await ws.send(data)
        else:
            offset = 0
            while offset < len(data):
                await ws.send(data[offset : offset + MAX_FRAME_SIZE])
                offset += MAX_FRAME_SIZE

    def send_message(self, msg: str) -> None:
        """Fire-and-forget text message. Queues a coroutine on the running loop."""
        try:
            loop = asyncio.get_running_loop()
            loop.create_task(self.send_message_async(msg))
        except RuntimeError:
            asyncio.run(self.send_message_async(msg))

    async def send_message_async(self, msg: str) -> None:
        ws = await self._ensure_connected()
        await ws.send(msg)

    def close(self) -> None:
        """Fire-and-forget close."""
        try:
            loop = asyncio.get_running_loop()
            loop.create_task(self.close_async())
        except RuntimeError:
            asyncio.run(self.close_async())

    async def close_async(self) -> None:
        if self._ws is not None and self._connected:
            await self._ws.close()
            self._connected = False


class ChannelReader:
    """WebSocket-backed reader for streaming binary data and text messages."""

    def __init__(self, engine_ws_base: str, ref: StreamChannelRef) -> None:
        self._url = build_channel_url(engine_ws_base, ref.channel_id, ref.access_key, "read")
        self._ws: ClientConnection | None = None
        self._connected = False
        self._lock = asyncio.Lock()
        self._message_callbacks: list[Callable[[str], Any]] = []
        self.stream = ReadableStream(self)

    async def _ensure_connected(self) -> ClientConnection:
        if self._ws is not None and self._connected:
            return self._ws
        async with self._lock:
            if self._ws is not None and self._connected:
                return self._ws
            self._ws = await websockets.connect(self._url)
            self._connected = True
            return self._ws

    def on_message(self, callback: Callable[[str], Any]) -> None:
        self._message_callbacks.append(callback)

    async def __aiter__(self) -> AsyncIterator[bytes]:
        """Async iterator that yields binary chunks and dispatches text messages to callbacks."""
        ws = await self._ensure_connected()
        try:
            async for message in ws:
                if isinstance(message, bytes):
                    yield message
                else:
                    for cb in self._message_callbacks:
                        try:
                            cb(message)
                        except Exception:
                            log.exception("Error in channel message callback")
        except websockets.ConnectionClosed:
            pass

    async def read_all(self) -> bytes:
        """Read the entire stream into a single bytes object."""
        chunks: list[bytes] = []
        async for chunk in self:
            chunks.append(chunk)
        return b"".join(chunks)

    async def close_async(self) -> None:
        if self._ws is not None and self._connected:
            await self._ws.close()
            self._connected = False
