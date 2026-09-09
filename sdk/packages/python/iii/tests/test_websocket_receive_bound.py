"""Real WebSocket receive-bound regressions for the Python SDK connection.

These tests exercise _do_connect without registration or reconnection machinery.
Full worker/Engine request-and-response qualification is separate evidence.
"""

import asyncio
import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock

from websockets.asyncio.server import serve
from websockets.exceptions import ConnectionClosedError

from iii.iii import III


class WebSocketReceiveBoundTests(unittest.IsolatedAsyncioTestCase):
    async def connect(self, port):
        client = object.__new__(III)
        client._address = f"ws://127.0.0.1:{port}"
        client._options = SimpleNamespace(headers=None)
        client._running = False
        client._on_connected = AsyncMock()
        client._ws = None
        await client._do_connect()
        self.assertIsNotNone(client._ws)
        client._on_connected.assert_awaited_once()
        return client._ws

    async def test_messages_larger_than_library_default_round_trip(self):
        async def echo(ws):
            async for message in ws:
                await ws.send(message)

        async with serve(echo, "127.0.0.1", 0, max_size=16 * 1024 * 1024) as server:
            ws = await self.connect(server.sockets[0].getsockname()[1])
            try:
                for size in (1024 * 1024 + 1, 8_000_000):
                    with self.subTest(bytes=size):
                        message = "x" * size
                        await ws.send(message)
                        self.assertEqual(await asyncio.wait_for(ws.recv(), 5), message)
            finally:
                await ws.close()

    async def test_receive_bound_remains_finite(self):
        async def oversized(ws):
            await ws.send("x" * (16 * 1024 * 1024 + 1))
            await ws.wait_closed()

        async with serve(oversized, "127.0.0.1", 0, compression=None) as server:
            ws = await self.connect(server.sockets[0].getsockname()[1])
            try:
                with self.assertRaises(ConnectionClosedError) as caught:
                    await asyncio.wait_for(ws.recv(), 5)
                self.assertIsNotNone(caught.exception.sent)
                self.assertEqual(caught.exception.sent.code, 1009)
            finally:
                await ws.close()


if __name__ == "__main__":
    unittest.main()
