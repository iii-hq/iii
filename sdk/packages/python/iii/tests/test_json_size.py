"""complete serialized-envelope limits, not payload character counts."""

import asyncio
import json
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest

from iii.errors import InvocationError
from iii.iii import _MAX_JSON_FRAME_BYTES, III


def client():
    sdk = III.__new__(III)
    sdk._fatal_error = None
    sdk._queue = []
    sdk._ws = SimpleNamespace(state=SimpleNamespace(name="OPEN"), send=AsyncMock())
    return sdk


@pytest.mark.parametrize("delta", [-1, 0, 1])
def test_complete_envelope_inclusive_boundary(delta):
    sdk = client()
    msg = {"type": "invokefunction", "data": ""}
    overhead = len(json.dumps(msg).encode())
    msg["data"] = "x" * (_MAX_JSON_FRAME_BYTES - overhead + delta)
    if delta > 0:
        with pytest.raises(InvocationError, match="payload_too_large"):
            sdk._prepare_json(msg)
    else:
        assert len(sdk._prepare_json(msg).encode()) == _MAX_JSON_FRAME_BYTES + delta


def test_unicode_and_escapes_count_serialized_bytes():
    sdk = client()
    msg = {"type": "invokefunction", "data": ('é😀\n"\\' * 700000)}
    assert len(msg["data"]) < _MAX_JSON_FRAME_BYTES
    assert len(json.dumps(msg).encode()) > _MAX_JSON_FRAME_BYTES
    with pytest.raises(InvocationError, match="payload_too_large"):
        sdk._prepare_json(msg)


@pytest.mark.asyncio
async def test_result_substitution_and_same_transport_small_send():
    sdk = client()
    socket = sdk._ws
    await sdk._send(
        {
            "type": "invocationresult",
            "invocation_id": "inv-1",
            "function_id": "f",
            "result": "x" * _MAX_JSON_FRAME_BYTES,
        }
    )
    fallback = json.loads(socket.send.call_args.args[0])
    assert fallback["invocation_id"] == "inv-1"
    assert fallback["error"]["code"] == "payload_too_large"
    assert "result" not in fallback
    await sdk._send({"type": "invocationresult", "invocation_id": "inv-2", "function_id": "f", "result": 1})
    assert sdk._ws is socket
    assert socket.send.call_count == 2


def test_preconnection_enqueue_rejects_arguments_and_substitutes_results():
    sdk = client()
    with pytest.raises(InvocationError):
        sdk._enqueue({"type": "invokefunction", "data": "x" * _MAX_JSON_FRAME_BYTES})
    assert sdk._queue == []
    sdk._enqueue(
        {"type": "invocationresult", "invocation_id": "i", "function_id": "f", "result": "x" * _MAX_JSON_FRAME_BYTES}
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
    with pytest.raises(InvocationError, match="payload_too_large"):
        await sdk.trigger_async({"function_id": "f", "payload": "x" * _MAX_JSON_FRAME_BYTES})
    assert sdk._pending == {}
    sdk._ws.send.assert_not_called()


@pytest.mark.asyncio
@pytest.mark.parametrize("invocation_id", [None, "queued-call", 123])
async def test_flush_skips_permanent_rejection_and_sends_following_message(invocation_id):
    sdk = client()
    sdk._worker_id = None
    sdk._trigger_types = {}
    sdk._functions = {}
    sdk._triggers = {}
    sdk._pending = {}
    sdk._reconnect_attempt = 0
    sdk._set_connection_state = lambda state: None
    sdk._register_worker_metadata = lambda: None
    sdk._receive_loop = AsyncMock()
    future = asyncio.get_running_loop().create_future()
    sdk._pending["queued-call"] = SimpleNamespace(future=future)
    rejected = {"type": "invokefunction", "data": "x" * _MAX_JSON_FRAME_BYTES}
    if invocation_id is not None:
        rejected["invocation_id"] = invocation_id
    sdk._queue = [
        rejected,
        {"type": "invocationresult", "invocation_id": "i", "function_id": "f", "result": "x" * _MAX_JSON_FRAME_BYTES},
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
