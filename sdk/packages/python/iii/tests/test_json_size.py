"""complete serialized-envelope limits, not payload character counts."""

import asyncio
import base64
import hashlib
import json
import struct
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest
import websockets

from iii.errors import InvocationError
from iii.iii import (
    _JSON_FRAGMENT_BYTES,
    _MAX_JSON_FRAME_BYTES,
    _MAX_JSON_OUTBOUND_BYTES,
    III,
    _json_fragments,
    _ObserveHost,
)


def compact(value):
    """JSON text as ``JSON.stringify`` and ``serde_json`` write it: compact, non-ASCII as UTF-8."""
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))


def client():
    sdk = III.__new__(III)
    sdk._fatal_error = None
    sdk._queue = []
    sdk._ws = SimpleNamespace(state=SimpleNamespace(name="OPEN"), send=AsyncMock())
    # observe state (a bare instance skips __init__)
    sdk._functions = {}
    sdk._engine_features = frozenset()
    sdk._registered_on_connection = False
    sdk._connection_epoch = 0
    sdk._held_observe_frames = []
    sdk._observers, sdk._observe_sent = {}, {}
    sdk._observe_host = _ObserveHost(sdk)
    sdk._worker_added = sdk._worker_removed = None
    sdk._added_hooks, sdk._removed_hooks, sdk._match_hooks, sdk._deliver_hooks = {}, {}, {}, {}
    sdk._hook_queues = {}
    sdk._standing_by = set()
    # A loop that never runs: registrations go out with the replay on connect.
    sdk._loop = SimpleNamespace(call_soon_threadsafe=lambda *args: None)
    return sdk


@pytest.mark.parametrize("delta", [-1, 0, 1])
def test_complete_envelope_inclusive_boundary(delta):
    sdk = client()
    msg = {"type": "invokefunction", "data": ""}
    overhead = len(compact(msg).encode())
    msg["data"] = "x" * (_MAX_JSON_FRAME_BYTES - overhead + delta)
    payload = sdk._prepare_json(msg)
    assert len(payload.encode()) == _MAX_JSON_FRAME_BYTES + delta
    assert payload == compact(msg)


def test_unicode_and_escapes_count_serialized_bytes():
    sdk = client()
    accepted = {"type": "invokefunction", "data": ('é😀\n"\\' * 1_500_000)}
    assert len(accepted["data"]) < _MAX_JSON_FRAME_BYTES
    payload = sdk._prepare_json(accepted)
    assert len(payload.encode("utf-8")) > _MAX_JSON_FRAME_BYTES
    assert json.loads(payload)["data"] == accepted["data"]

    expanded = {
        "type": "invokefunction",
        "function_id": "f",
        "data": ('é😀\n"\\' * 6_000_000),
    }
    serialized_size = len(compact(expanded).encode("utf-8"))
    assert len(expanded["data"].encode("utf-8")) < _MAX_JSON_OUTBOUND_BYTES
    assert serialized_size > _MAX_JSON_OUTBOUND_BYTES
    with pytest.raises(InvocationError) as raised:
        sdk._prepare_json(expanded)
    assert raised.value.code == "payload_too_large"
    assert raised.value.message == (
        f"Serialized JSON envelope is {serialized_size} bytes; limit is "
        f"{_MAX_JSON_OUTBOUND_BYTES} bytes. Use channels for larger payloads: "
        "https://iii.dev/docs/creating-workers/channels#payload-size"
    )


# `written` and `node_bytes`: how Node's JSON.stringify writes the same message.
@pytest.mark.parametrize(
    ("text", "written", "node_bytes"),
    [
        ("Ação é ótima, naïve café", "Ação é ótima, naïve café", 125),
        ("中文字符テスト한국어", "中文字符テスト한국어", 125),
        ("😀🎉👍🏽", "😀🎉👍🏽", 111),
        ('é😀中\n"\\\u0001\u2028\x7f', 'é😀中\\n\\"\\\\\\u0001\u2028\x7f', 120),
    ],
    ids=["accents", "cjk", "emoji", "escapes"],
)
def test_envelope_is_compact_utf8_json_like_the_other_sdks(text, written, node_bytes):
    sdk = client()
    msg = {"type": "invokefunction", "invocation_id": "i", "function_id": "f", "data": {"text": text, "list": [1, 2]}}
    payload = sdk._prepare_json(msg)
    assert payload == (
        f'{{"type":"invokefunction","invocation_id":"i","function_id":"f","data":{{"text":"{written}","list":[1,2]}}}}'
    )
    assert len(payload.encode("utf-8")) == node_bytes
    assert json.loads(payload) == msg


def test_non_ascii_payload_counts_its_utf8_bytes_against_the_limit():
    sdk = client()
    # 30 MiB of emoji: 90 MiB with \u escapes, 30 MiB (as in Node and Rust) as UTF-8.
    emoji = {"type": "invokefunction", "invocation_id": "i", "function_id": "f", "data": "😀" * (30 * 1024 * 1024 // 4)}
    assert len(sdk._prepare_json(emoji).encode("utf-8")) == 31_457_353

    accented = {"type": "invokefunction", "invocation_id": "inv-64", "function_id": "f", "data": ""}
    room = _MAX_JSON_OUTBOUND_BYTES - len(compact(accented).encode("utf-8"))
    accented["data"] = "é" * (room // 2) + "x" * (room % 2)
    assert len(sdk._prepare_json(accented).encode("utf-8")) == _MAX_JSON_OUTBOUND_BYTES
    accented["data"] += "é"
    with pytest.raises(InvocationError) as raised:
        sdk._prepare_json(accented)
    assert raised.value.message.startswith(
        f"Serialized JSON envelope is {_MAX_JSON_OUTBOUND_BYTES + 2} bytes; limit is {_MAX_JSON_OUTBOUND_BYTES} bytes."
    )


@pytest.mark.asyncio
async def test_lone_surrogates_stay_escaped_and_the_message_is_still_sent():
    sdk = client()
    broken = b"caf\xe9".decode("utf-8", "surrogateescape")  # "caf\udce9": no UTF-8 for it
    msg = {"type": "invokefunction", "function_id": "f", "data": {broken: "é😀" + broken + chr(0xD83D) + chr(0xDE00)}}
    payload = sdk._prepare_json(msg)
    # Escaped as before (and as JSON.stringify writes a lone surrogate); the rest is UTF-8.
    assert payload == (
        '{"type":"invokefunction","function_id":"f","data":{"caf\\udce9":"é😀caf\\udce9\\ud83d\\ude00"}}'
    )
    payload.encode("utf-8")
    assert json.loads(payload)["data"] == {broken: "é😀" + broken + "😀"}

    await sdk._send(msg)
    sdk._ws.send.assert_awaited_once_with(payload)


def test_nan_and_infinity_are_written_as_before():
    sdk = client()
    msg = {"type": "invocationresult", "invocation_id": "i", "result": [float("nan"), float("inf"), -float("inf")]}
    assert sdk._prepare_json(msg) == '{"type":"invocationresult","invocation_id":"i","result":[NaN,Infinity,-Infinity]}'


@pytest.mark.asyncio
async def test_reporter_send_frame_routes_observe_reports_through_fragmenter():
    sdk = client()
    sdk._ws = SimpleNamespace(
        state=SimpleNamespace(name="OPEN"),
        send=AsyncMock(),
        transport=None,
    )
    sdk._inflight_bytes = 0
    sdk._frame_sends = set()
    sdk._background_done = lambda task: None
    payload = json.dumps({"type": "invocationsobserved", "items": "x" * (40 * 1024 * 1024)})

    assert sdk._send_frame(payload)
    await asyncio.gather(*sdk._frame_sends)

    sent = sdk._ws.send.await_args.args[0]
    assert not isinstance(sent, str)
    fragments = list(sent)
    assert "".join(fragments) == payload
    assert all(len(fragment.encode("utf-8")) <= _JSON_FRAGMENT_BYTES for fragment in fragments)


@pytest.mark.asyncio
async def test_single_frame_threshold_keeps_one_text_frame():
    sdk = client()
    message = {"type": "test", "data": ""}
    overhead = len(json.dumps(message).encode("utf-8"))
    message["data"] = "x" * (_MAX_JSON_FRAME_BYTES - overhead)
    payload = json.dumps(message)
    assert len(payload.encode("utf-8")) == _MAX_JSON_FRAME_BYTES

    await sdk._send_json(sdk._ws, payload)

    sdk._ws.send.assert_awaited_once_with(payload)



@pytest.mark.parametrize("size", [_MAX_JSON_FRAME_BYTES + 1, 40 * 1024 * 1024, _MAX_JSON_OUTBOUND_BYTES])
def test_fragment_helper_respects_utf8_boundaries_and_frame_budget(size):
    payload = "é😀" * (size // len("é😀".encode()))
    encoded = payload.encode("utf-8")
    fragments = list(_json_fragments(encoded))
    assert len(fragments) > 1
    assert all(len(fragment.encode("utf-8")) <= _JSON_FRAGMENT_BYTES for fragment in fragments)
    assert "".join(fragments).encode("utf-8") == encoded


@pytest.mark.asyncio
async def test_fragmented_messages_round_trip_through_websockets_server():
    server_result = asyncio.get_running_loop().create_future()
    expected_sizes = (_MAX_JSON_FRAME_BYTES + 1, 40 * 1024 * 1024, _MAX_JSON_OUTBOUND_BYTES)

    async def handler(ws):
        try:
            received = []
            for _ in expected_sizes:
                fragments = []
                async for fragment in ws.recv_streaming():
                    fragments.append(fragment)
                message = "".join(fragments)
                received.append(
                    {
                        "size": len(message.encode("utf-8")),
                        "count": len(fragments),
                        "fragment_sizes": [len(fragment.encode("utf-8")) for fragment in fragments],
                        "type": json.loads(message)["type"],
                    }
                )
            if not server_result.done():
                server_result.set_result(received)
        except Exception as error:
            if not server_result.done():
                server_result.set_exception(error)

    async with websockets.serve(handler, "127.0.0.1", 0, max_size=_MAX_JSON_OUTBOUND_BYTES + 1024) as server:
        port = server.sockets[0].getsockname()[1]
        async with websockets.connect(f"ws://127.0.0.1:{port}", max_size=_MAX_JSON_OUTBOUND_BYTES + 1024) as ws:
            for size in expected_sizes:
                overhead = len(json.dumps({"type": "test", "data": ""}, ensure_ascii=False).encode())
                prefix = "é😀" * 1000
                remaining = size - overhead - len(prefix.encode("utf-8"))
                payload = json.dumps(
                    {"type": "test", "data": prefix + "x" * remaining}, ensure_ascii=False
                )
                assert len(payload.encode("utf-8")) == size
                await III._send_json(III.__new__(III), ws, payload)
        received = await asyncio.wait_for(server_result, timeout=30)

    assert [item["size"] for item in received] == list(expected_sizes)
    assert [item["type"] for item in received] == ["test"] * len(expected_sizes)
    for item in received:
        assert item["count"] > 1
        assert all(size <= _JSON_FRAGMENT_BYTES for size in item["fragment_sizes"])


class _RawBackpressureServer:
    """Raw WebSocket peer that withholds reads until its gate opens."""

    def __init__(self):
        self.gate = asyncio.Event()
        self.messages_future = asyncio.get_running_loop().create_future()
        self.first_message_future = asyncio.get_running_loop().create_future()
        self.messages = []
        self.frames = []
        self.json_messages = []
        self.connections = 0
        self.reconnected_future = asyncio.get_running_loop().create_future()
        self.tasks = []

    async def start(self):
        self.server = await asyncio.start_server(self._handle, "127.0.0.1", 0)
        return self.server.sockets[0].getsockname()[1]

    async def _handle(self, reader, writer):
        self.connections += 1
        if self.connections >= 2 and not self.reconnected_future.done():
            self.reconnected_future.set_result(self.connections)
        self.tasks.append(asyncio.current_task())
        current_type = None
        try:
            request = await reader.readuntil(b"\r\n\r\n")
            key = next(
                line.split(b":", 1)[1].strip()
                for line in request.split(b"\r\n")
                if line.lower().startswith(b"sec-websocket-key:")
            )
            accept = base64.b64encode(
                hashlib.sha1(key + b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11").digest()
            )
            writer.write(
                b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
                b"Sec-WebSocket-Accept: " + accept + b"\r\n\r\n"
            )
            await writer.drain()
            while True:
                await self.gate.wait()
                head = await reader.readexactly(2)
                fin, opcode, length = bool(head[0] & 0x80), head[0] & 0x0F, head[1] & 0x7F
                if length == 126:
                    length = struct.unpack("!H", await reader.readexactly(2))[0]
                elif length == 127:
                    length = struct.unpack("!Q", await reader.readexactly(8))[0]
                mask = await reader.readexactly(4) if head[1] & 0x80 else b"\0\0\0\0"
                frame = await reader.readexactly(length)
                self.frames.append((fin, opcode, length))
                if opcode == 1 and length < 4096:
                    data = bytes(byte ^ mask[index & 3] for index, byte in enumerate(frame))
                    try:
                        parsed = json.loads(data)
                        current_type = parsed.get("function_id") or parsed.get("type")
                    except (UnicodeDecodeError, json.JSONDecodeError, AttributeError):
                        current_type = None
                elif opcode == 1:
                    current_type = "large"
                elif opcode & 0x08:
                    continue
                if fin and opcode in (0, 1):
                    self.messages.append(current_type)
                    if not self.first_message_future.done():
                        self.first_message_future.set_result((current_type, list(self.frames)))
                    if current_type in {"large", "small"}:
                        self.json_messages.append(current_type)
                    completed_type = current_type
                    current_type = None
                    if completed_type in {"large", "small"} and not self.messages_future.done():
                        self.messages_future.set_result(list(self.json_messages))
        except asyncio.CancelledError:
            raise
        except asyncio.IncompleteReadError:
            pass
        except Exception as error:
            if not self.messages_future.done():
                self.messages_future.set_exception(error)
        finally:
            writer.close()

    async def close(self):
        self.gate.set()
        self.server.close()
        await self.server.wait_closed()
        for task in self.tasks:
            task.cancel()
        await asyncio.gather(*self.tasks, return_exceptions=True)


@pytest.mark.asyncio
async def test_cancelled_fragmented_send_waiting_for_another_preserves_first_message():
    server = _RawBackpressureServer()
    port = await server.start()
    ws = await websockets.connect(f"ws://127.0.0.1:{port}", compression=None)
    first_payload = json.dumps({"type": "A", "data": "x" * (40 * 1024 * 1024)})
    second_payload = json.dumps({"type": "B", "data": "y" * (20 * 1024 * 1024)})
    first = asyncio.create_task(III._send_json(III.__new__(III), ws, first_payload))
    try:
        deadline = asyncio.get_running_loop().time() + 10
        while ws.send_in_progress is None and asyncio.get_running_loop().time() < deadline:
            await asyncio.sleep(0.005)
        assert ws.send_in_progress is not None
        assert not first.done(), "raw peer must withhold reads and apply backpressure"

        second = asyncio.create_task(
            asyncio.wait_for(III._send_json(III.__new__(III), ws, second_payload), timeout=0.1)
        )
        with pytest.raises(asyncio.TimeoutError):
            await second
        assert not first.done(), "cancelling the waiter must not interrupt the active send"
        assert ws.state.name == "OPEN"

        server.gate.set()
        await asyncio.wait_for(first, timeout=30)
        message_type, frames = await asyncio.wait_for(server.first_message_future, timeout=30)
        assert message_type == "large"
        assert ws.state.name == "OPEN"
        assert server.connections == 1
        assert len(frames) > 1
        assert frames[0][1] == 1 and not frames[0][0]
        assert frames[-1][1] == 0 and frames[-1][0]
    finally:
        server.gate.set()
        if ws.state.name == "OPEN":
            ws.transport.abort()
        await server.close()


@pytest.mark.asyncio
async def test_cancelled_fragmented_send_after_fin_does_not_abort_connection():
    fin_written = asyncio.Event()
    let_send_finish = asyncio.Event()
    keep_server_open = asyncio.Event()
    received = asyncio.get_running_loop().create_future()
    received_small = asyncio.get_running_loop().create_future()

    async def handler(ws):
        try:
            fragments = []
            async for fragment in ws.recv_streaming():
                fragments.append(fragment)
            if not received.done():
                received.set_result("".join(fragments))
            small = await ws.recv()
            if not received_small.done():
                received_small.set_result(small)
            await keep_server_open.wait()
        except Exception as error:
            if not received.done():
                received.set_exception(error)
            if not received_small.done():
                received_small.set_exception(error)

    async with websockets.serve(handler, "127.0.0.1", 0, max_size=_MAX_JSON_OUTBOUND_BYTES + 1024) as server:
        port = server.sockets[0].getsockname()[1]
        async with websockets.connect(f"ws://127.0.0.1:{port}", compression=None) as ws:
            original_send_context = ws.send_context

            class DelayedContext:
                def __init__(self, context):
                    self.context = context

                async def __aenter__(self):
                    return await self.context.__aenter__()

                async def __aexit__(self, exc_type, exc, tb):
                    result = await self.context.__aexit__(exc_type, exc, tb)
                    if fin_written.is_set():
                        await let_send_finish.wait()
                    return result

            def send_context(*args, **kwargs):
                return DelayedContext(original_send_context(*args, **kwargs))

            original_continuation = ws.protocol.send_continuation

            def send_continuation(data, fin):
                original_continuation(data, fin)
                if fin:
                    fin_written.set()

            ws.send_context = send_context
            ws.protocol.send_continuation = send_continuation
            message = json.dumps({"type": "fin", "data": "x" * (_MAX_JSON_FRAME_BYTES + 1)})
            task = asyncio.create_task(III._send_json(III.__new__(III), ws, message))
            try:
                await asyncio.wait_for(fin_written.wait(), timeout=10)
                assert not ws.protocol.expect_continuation_frame
                task.cancel()
                with pytest.raises(asyncio.CancelledError):
                    await task
                assert ws.state.name == "OPEN"
                let_send_finish.set()
                assert await asyncio.wait_for(received, timeout=10) == message
                small = json.dumps({"type": "small-after-fin", "data": 1})
                await III._send_json(III.__new__(III), ws, small)
                assert await asyncio.wait_for(received_small, timeout=10) == small
                assert ws.state.name == "OPEN"
            finally:
                let_send_finish.set()
                keep_server_open.set()
                ws.protocol.send_continuation = original_continuation
                ws.send_context = original_send_context


@pytest.mark.asyncio
async def test_cancelled_sdk_invocation_waiting_for_fragmented_send_preserves_active_send(monkeypatch):
    from iii.iii_constants import InitOptions

    monkeypatch.setattr("iii_helpers.observability.telemetry.init_otel", lambda **kwargs: None)
    monkeypatch.setattr("iii_helpers.observability.telemetry.attach_event_loop", lambda loop: None)
    server = _RawBackpressureServer()
    port = await server.start()
    sdk = III(f"ws://127.0.0.1:{port}", InitOptions(enable_metrics_reporting=False))
    try:
        await asyncio.to_thread(sdk._wait_until_connected)
        # Drain setup/registration traffic before creating the two user sends.
        server.gate.set()
        await asyncio.sleep(0.1)
        server.gate.clear()
        first = {"function_id": "A", "payload": "x" * (40 * 1024 * 1024), "action": {"type": "void"}}
        first_task = asyncio.run_coroutine_threadsafe(sdk.call_async(first), sdk._loop)
        await asyncio.sleep(0.4)
        assert sdk._ws is not None and sdk._ws.send_in_progress is not None

        second = {"function_id": "B", "payload": "y" * (20 * 1024 * 1024), "action": {"type": "void"}}
        second_task = asyncio.run_coroutine_threadsafe(
            asyncio.wait_for(sdk.call_async(second), timeout=0.1), sdk._loop
        )
        with pytest.raises(asyncio.TimeoutError):
            await asyncio.wait_for(asyncio.wrap_future(second_task), timeout=10)
        assert sdk._ws is not None and sdk._ws.state.name == "OPEN"

        # The first invocation wasn't cancelled and must complete on this socket.
        server.gate.set()
        await asyncio.wrap_future(first_task)
        messages = await asyncio.wait_for(server.messages_future, timeout=30)
        assert messages == ["large"]
        assert server.frames[-1][0] and server.frames[-1][1] == 0
        assert sdk._ws is not None and sdk._ws.state.name == "OPEN"
        assert server.connections == 1
    finally:
        server.gate.set()
        if sdk._ws is not None and sdk._ws.state.name == "OPEN":
            sdk._loop.call_soon_threadsafe(sdk._ws.transport.abort)
        shutdown = asyncio.run_coroutine_threadsafe(sdk.shutdown_async(), sdk._loop)
        try:
            await asyncio.wait_for(asyncio.wrap_future(shutdown), timeout=15)
        except Exception:
            pass
        await server.close()


@pytest.mark.asyncio
async def test_cancelled_sdk_invocation_aborts_and_reconnects_under_backpressure(monkeypatch):
    from iii.iii_constants import InitOptions

    monkeypatch.setattr("iii_helpers.observability.telemetry.init_otel", lambda **kwargs: None)
    monkeypatch.setattr("iii_helpers.observability.telemetry.attach_event_loop", lambda loop: None)
    server = _RawBackpressureServer()
    port = await server.start()
    sdk = III(f"ws://127.0.0.1:{port}", InitOptions(enable_metrics_reporting=False))
    try:
        await asyncio.to_thread(sdk._wait_until_connected)
        server.gate.clear()
        await asyncio.sleep(0.1)
        invocation = {
            "function_id": "large",
            "payload": "x" * (40 * 1024 * 1024),
            "action": {"type": "void"},
        }
        timed_out = asyncio.run_coroutine_threadsafe(
            asyncio.wait_for(sdk.call_async(invocation), timeout=0.1), sdk._loop
        )
        with pytest.raises(asyncio.TimeoutError):
            await asyncio.wait_for(asyncio.wrap_future(timed_out), timeout=10)

        # Cancelling mid-fragment aborts the first transport; the SDK's normal
        # receive-loop/reconnect path must establish a fresh usable socket.
        server.gate.set()
        await asyncio.wait_for(server.reconnected_future, timeout=10)
        deadline = asyncio.get_running_loop().time() + 10
        while (sdk._ws is None or sdk._ws.state.name != "OPEN") and asyncio.get_running_loop().time() < deadline:
            await asyncio.sleep(0.05)
        assert sdk._ws is not None and sdk._ws.state.name == "OPEN"
        small = {"function_id": "small", "payload": {}, "action": {"type": "void"}}
        small_send = asyncio.run_coroutine_threadsafe(sdk.call_async(small), sdk._loop)
        await asyncio.wait_for(asyncio.wrap_future(small_send), timeout=20)
        messages = await asyncio.wait_for(server.messages_future, timeout=20)
        assert messages == ["small"]
        assert sdk._ws is not None and sdk._ws.state.name == "OPEN"
        assert server.connections >= 2
    finally:
        server.gate.set()
        if sdk._ws is not None and sdk._ws.state.name == "OPEN":
            sdk._loop.call_soon_threadsafe(sdk._ws.transport.abort)
        shutdown = asyncio.run_coroutine_threadsafe(sdk.shutdown_async(), sdk._loop)
        try:
            await asyncio.wait_for(asyncio.wrap_future(shutdown), timeout=15)
        except Exception:
            pass
        await server.close()


@pytest.mark.asyncio
async def test_concurrent_websocket_send_cannot_interleave_fragmented_message():
    server_result = asyncio.get_running_loop().create_future()

    async def handler(ws):
        try:
            first_fragments = []
            async for fragment in ws.recv_streaming():
                first_fragments.append(fragment)
            second = await ws.recv()
            server_result.set_result(
                {
                    "first": "".join(first_fragments),
                    "first_fragment_count": len(first_fragments),
                    "first_fragment_sizes": [len(fragment.encode("utf-8")) for fragment in first_fragments],
                    "second": second,
                }
            )
        except Exception as error:
            if not server_result.done():
                server_result.set_exception(error)

    async with websockets.serve(handler, "127.0.0.1", 0, max_size=_MAX_JSON_OUTBOUND_BYTES) as server:
        port = server.sockets[0].getsockname()[1]
        async with websockets.connect(f"ws://127.0.0.1:{port}") as ws:
            large = json.dumps({"type": "large", "data": "x" * _MAX_JSON_FRAME_BYTES})
            small = json.dumps({"type": "small", "data": 1})
            large_task = asyncio.create_task(III._send_json(III.__new__(III), ws, large))
            await asyncio.sleep(0)
            await asyncio.gather(large_task, ws.send(small))
        received = await asyncio.wait_for(server_result, timeout=30)

    assert json.loads(received["first"])["type"] == "large"
    assert json.loads(received["second"])["type"] == "small"
    assert received["first_fragment_count"] > 1
    assert all(size <= _JSON_FRAGMENT_BYTES for size in received["first_fragment_sizes"])


@pytest.mark.asyncio
async def test_outbound_limit_acceptance_and_exact_over_limit_error():
    sdk = client()
    invocation = {"type": "invokefunction", "invocation_id": "inv-64", "function_id": "f", "data": ""}
    overhead = len(compact(invocation).encode())
    invocation["data"] = "x" * (_MAX_JSON_OUTBOUND_BYTES - overhead)
    assert len(sdk._prepare_json(invocation).encode()) == _MAX_JSON_OUTBOUND_BYTES
    invocation["data"] += "x"
    with pytest.raises(InvocationError) as raised:
        sdk._prepare_json(invocation)
    assert raised.value.code == "payload_too_large"
    assert raised.value.invocation_id == "inv-64"
    assert raised.value.function_id == "f"
    assert raised.value.message == (
        f"Serialized JSON envelope is {_MAX_JSON_OUTBOUND_BYTES + 1} bytes; limit is "
        f"{_MAX_JSON_OUTBOUND_BYTES} bytes. Use channels for larger payloads: "
        "https://iii.dev/docs/creating-workers/channels#payload-size"
    )


@pytest.mark.asyncio
async def test_oversized_invocation_keeps_pending_cleanup_guarantee():
    sdk = client()
    sdk._loop = asyncio.get_running_loop()
    sdk._pending = {}
    sdk._options = SimpleNamespace(invocation_timeout_ms=1000)
    sdk._invocation_namespace = lambda *args: None
    sdk._inject_traceparent = lambda: None
    sdk._inject_baggage = lambda: None
    with pytest.raises(InvocationError, match="payload_too_large"):
        await sdk.call_async({"function_id": "f", "payload": "x" * (_MAX_JSON_OUTBOUND_BYTES + 1)})
    assert sdk._pending == {}
    sdk._ws.send.assert_not_called()


@pytest.mark.asyncio
async def test_result_substitution_and_same_transport_small_send():
    sdk = client()
    socket = sdk._ws
    await sdk._send(
        {
            "type": "invocationresult",
            "invocation_id": "inv-1",
            "function_id": "f",
            "result": "x" * (_MAX_JSON_OUTBOUND_BYTES + 1),
        }
    )
    fallback = json.loads(socket.send.call_args.args[0])
    assert fallback["invocation_id"] == "inv-1"
    assert fallback["error"]["code"] == "payload_too_large"
    assert "result" not in fallback
    await sdk._send({"type": "invocationresult", "invocation_id": "inv-2", "function_id": "f", "result": 1})
    assert sdk._ws is socket
    assert socket.send.call_count == 2


@pytest.mark.parametrize(
    ("features", "fields", "expected"),
    [
        # observe.v2: the error activates no observer, whatever the function's hooks
        (["observe.v1", "observe.v2"], {}, {"matched": []}),
        (["observe.v1", "observe.v2"], {"matched": ["o-1", "o-2"]}, {"matched": []}),
        (["observe.v1", "observe.v2"], {"delivered": True}, {"delivered": True}),
        # an engine without observe.v2 gets the bare error
        (["observe.v1"], {"matched": ["o-1"]}, {}),
        (["observe.v1"], {"delivered": True}, {}),
    ],
)
def test_an_answer_too_large_to_send_activates_no_observer(features, fields, expected):
    sdk = client()
    sdk._engine_features = frozenset(features)
    answer = {
        "type": "invocationresult",
        "invocation_id": "inv-1",
        "function_id": "f",
        "result": "x" * (_MAX_JSON_OUTBOUND_BYTES + 1),
        **fields,
    }
    fallback = json.loads(sdk._prepare_json(answer))
    assert {k: v for k, v in fallback.items() if k in ("matched", "delivered")} == expected
    assert fallback["error"]["code"] == "payload_too_large"
    assert "result" not in fallback


def test_preconnection_enqueue_rejects_arguments_and_substitutes_results():
    sdk = client()
    with pytest.raises(InvocationError):
        sdk._enqueue({"type": "invokefunction", "data": "x" * (_MAX_JSON_OUTBOUND_BYTES + 1)})
    assert sdk._queue == []
    sdk._enqueue(
        {
            "type": "invocationresult",
            "invocation_id": "i",
            "function_id": "f",
            "result": "x" * (_MAX_JSON_OUTBOUND_BYTES + 1),
        }
    )
    assert sdk._queue[0]["error"]["code"] == "payload_too_large"


@pytest.mark.asyncio
async def test_local_argument_rejection_removes_pending():
    sdk = client()
    sdk._loop = asyncio.get_running_loop()
    sdk._pending = {}
    sdk._options = SimpleNamespace(invocation_timeout_ms=1000)
    sdk._invocation_namespace = lambda *args: None
    sdk._inject_traceparent = lambda: None
    sdk._inject_baggage = lambda: None
    with pytest.raises(InvocationError, match="payload_too_large") as raised:
        await sdk.call_async({"function_id": "f", "payload": "x" * (_MAX_JSON_OUTBOUND_BYTES + 1)})
    assert raised.value.function_id == "f"
    assert isinstance(raised.value.invocation_id, str)
    assert raised.value.invocation_id
    assert sdk._pending == {}
    sdk._ws.send.assert_not_called()


@pytest.mark.asyncio
@pytest.mark.parametrize("invocation_id", [None, "queued-call", 123])
async def test_flush_skips_permanent_rejection_and_sends_following_message(invocation_id):
    sdk = client()
    sdk._worker_id = None
    sdk._functions = {}
    sdk._pending = {}
    sdk._reconnect_attempt = 0
    sdk._set_connection_state = lambda state: None
    sdk._register_worker_metadata = lambda: None
    sdk._receive_loop = AsyncMock()
    future = asyncio.get_running_loop().create_future()
    sdk._pending["queued-call"] = SimpleNamespace(future=future)
    rejected = {"type": "invokefunction", "data": "x" * _MAX_JSON_OUTBOUND_BYTES}
    if invocation_id is not None:
        rejected["invocation_id"] = invocation_id
    sdk._queue = [
        rejected,
        {
            "type": "invocationresult",
            "invocation_id": "i",
            "function_id": "f",
            "result": "x" * (_MAX_JSON_OUTBOUND_BYTES + 1),
        },
        {"type": "invocationresult", "invocation_id": "j", "function_id": "f", "result": 1},
    ]
    await sdk._on_connected()
    await sdk._receiver_task
    frames = [json.loads(call.args[0]) for call in sdk._ws.send.call_args_list]
    assert len(frames) == 2
    assert frames[0]["error"]["code"] == "payload_too_large"
    assert frames[1]["result"] == 1
    assert sdk._queue == []
    if invocation_id == "queued-call":
        assert sdk._pending == {}
        with pytest.raises(InvocationError, match="payload_too_large"):
            await future
    else:
        assert sdk._pending["queued-call"].future is future
        assert not future.done()
        future.cancel()


@pytest.mark.asyncio
async def test_connect_sets_explicit_protocol_message_receive_limit(monkeypatch):
    from iii.iii import _MAX_JSON_MESSAGE_BYTES

    sdk = client()
    sdk._address = "ws://127.0.0.1:0"
    sdk._options = SimpleNamespace(headers=None)
    sdk._on_connected = AsyncMock()
    connect = AsyncMock(return_value=sdk._ws)
    monkeypatch.setattr("iii.iii.websockets.connect", connect)
    await sdk._do_connect()
    assert connect.call_args.kwargs["max_size"] == _MAX_JSON_MESSAGE_BYTES
    assert _MAX_JSON_MESSAGE_BYTES == 67_108_864


@pytest.mark.asyncio
async def test_replay_transport_failure_does_not_requeue_or_reset_backoff():
    sdk = client()
    sdk._worker_id = None
    sdk._functions = {}
    sdk._pending = {}
    sdk._reconnect_attempt = 1
    sdk._set_connection_state = lambda state: None
    sdk._register_worker_metadata = lambda: None
    sdk._receive_loop = AsyncMock()
    socket = sdk._ws
    socket.state.name = "CLOSING"
    socket.send.side_effect = ConnectionError("replay socket closed")
    sdk._queue = [{"type": "invocationresult", "invocation_id": "small", "result": 1}]

    with pytest.raises(ConnectionError, match="replay socket closed"):
        await sdk._on_connected()

    socket.send.assert_awaited_once()
    assert json.loads(socket.send.call_args.args[0])["invocation_id"] == "small"
    assert sdk._queue == []
    assert sdk._reconnect_attempt == 1
    assert not hasattr(sdk, "_receiver_task")
    sdk._receive_loop.assert_not_called()


@pytest.mark.parametrize("invocation_id", [None, 123, "local-call"])
def test_local_size_error_correlates_only_valid_invocation_ids(invocation_id):
    sdk = client()
    message = {"type": "invokefunction", "function_id": "size::echo", "data": "x" * (_MAX_JSON_OUTBOUND_BYTES + 1)}
    if invocation_id is not None:
        message["invocation_id"] = invocation_id
    with pytest.raises(InvocationError) as raised:
        sdk._prepare_json(message)
    assert raised.value.code == "payload_too_large"
    assert raised.value.invocation_id == (invocation_id if isinstance(invocation_id, str) else None)
    assert raised.value.function_id == ("size::echo" if isinstance(invocation_id, str) else None)


def registration_client():
    sdk = client()
    sdk._ws.state.name = "CLOSING"
    sdk._options = SimpleNamespace(namespace=None)
    sdk._functions = {}
    return sdk


@pytest.mark.asyncio
async def test_registration_snapshot_survives_caller_mutation_and_replays_following_work():
    sdk = registration_client()
    schema = {"type": "object", "properties": {"nested": {"type": "string"}}}
    metadata = {"tags": ["original"], "nullable": None}
    sdk.register_function("size::echo", lambda value: value, metadata=metadata, request_format=schema)
    retained = sdk._functions["size::echo"].message
    assert retained.request_format is not schema
    assert retained.metadata is not metadata
    schema["properties"]["nested"]["type"] = "x" * (_MAX_JSON_OUTBOUND_BYTES + 1)
    metadata["tags"].append("mutated")
    sdk._worker_id = None
    sdk._pending = {}
    sdk._reconnect_attempt = 2
    sdk._set_connection_state = lambda state: None
    sdk._register_worker_metadata = lambda: None
    sdk._receive_loop = AsyncMock()
    sdk._queue = [{"type": "invocationresult", "invocation_id": "next", "result": 1}]
    sdk._ws.state.name = "OPEN"

    await sdk._on_connected()
    await sdk._receiver_task

    frames = [json.loads(call.args[0]) for call in sdk._ws.send.call_args_list]
    assert frames[0]["request_format"] == {"type": "object", "properties": {"nested": {"type": "string"}}}
    assert frames[0]["metadata"] == {"tags": ["original"], "nullable": None}
    assert frames[1]["invocation_id"] == "next"
    assert frames[1]["result"] == 1
    assert sdk._queue == []
    assert sdk._reconnect_attempt == 0
    sdk._receive_loop.assert_awaited_once()


def test_related_registration_snapshots_preserve_schemas_metadata_and_http_config():
    from iii_helpers.http import HttpInvocationConfig

    sdk = registration_client()
    schema = {"type": "object", "properties": {"value": {"type": "string"}}}
    metadata = {"nested": {"value": "original"}}

    def handler(value):
        return value

    sdk.register_function(
        "size::handler", handler, request_format=schema, response_format=schema, metadata=metadata, args_format=schema
    )
    invocation = HttpInvocationConfig(url="https://example.invalid/handler", headers={"X-Test": "original"})
    sdk.register_function("size::http", invocation, request_format=schema, metadata=metadata)
    schema["properties"]["value"]["type"] = "x" * (_MAX_JSON_OUTBOUND_BYTES + 1)
    metadata["nested"]["value"] = "mutated"
    invocation.headers["X-Test"] = "mutated"
    expected = {"type": "object", "properties": {"value": {"type": "string"}}}
    for registration in sdk._functions.values():
        assert registration.message.request_format == expected
        assert registration.message.metadata == {"nested": {"value": "original"}}
        sdk._prepare_json(registration.message)
    assert sdk._functions["size::handler"].message.response_format == expected
    assert sdk._functions["size::handler"].message.args_format == expected
    assert sdk._functions["size::http"].message.invocation.headers == {"X-Test": "original"}


def test_a_registration_too_large_to_send_is_refused_and_not_kept():
    sdk = registration_client()
    with pytest.raises(InvocationError, match="payload_too_large"):
        sdk.register_function("size::huge", lambda value: value, description="x" * (_MAX_JSON_OUTBOUND_BYTES + 1))
    assert "size::huge" not in sdk._functions
    assert sdk._queue == []
