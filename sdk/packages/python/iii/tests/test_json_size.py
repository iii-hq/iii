"""complete serialized-envelope limits, not payload character counts."""

import asyncio
import json
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest

from iii.errors import InvocationError
from iii.iii import _MAX_JSON_FRAME_BYTES, III
from iii.triggers import TriggerHandler


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
    with pytest.raises(InvocationError, match="payload_too_large") as raised:
        await sdk.trigger_async({"function_id": "f", "payload": "x" * _MAX_JSON_FRAME_BYTES})
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


@pytest.mark.asyncio
async def test_replay_transport_failure_does_not_requeue_or_reset_backoff():
    sdk = client()
    sdk._worker_id = None
    sdk._trigger_types = {}
    sdk._functions = {}
    sdk._triggers = {}
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
    message = {"type": "invokefunction", "function_id": "size::echo", "data": "x" * _MAX_JSON_FRAME_BYTES}
    if invocation_id is not None:
        message["invocation_id"] = invocation_id
    with pytest.raises(InvocationError) as raised:
        sdk._prepare_json(message)
    assert raised.value.code == "payload_too_large"
    assert raised.value.invocation_id == (invocation_id if isinstance(invocation_id, str) else None)
    assert raised.value.function_id == ("size::echo" if isinstance(invocation_id, str) else None)


class TestTriggerHandler(TriggerHandler):
    async def register_trigger(self, config):
        pass

    async def unregister_trigger(self, config):
        pass


def registration_client():
    sdk = client()
    sdk._ws.state.name = "CLOSING"
    sdk._options = SimpleNamespace(namespace=None)
    sdk._trigger_types = {}
    sdk._functions = {}
    sdk._triggers = {}
    return sdk


@pytest.mark.asyncio
async def test_trigger_snapshot_survives_caller_mutation_and_replays_following_work():
    sdk = registration_client()
    config = {"nested": {"value": "accepted"}}
    metadata = {"tags": ["original"], "nullable": None}
    sdk.register_trigger({"type": "custom", "function_id": "size::echo", "config": config, "metadata": metadata})
    retained = next(iter(sdk._triggers.values()))
    assert retained.config is not config
    assert retained.config["nested"] is not config["nested"]
    assert retained.metadata is not metadata
    config["nested"]["value"] = "x" * _MAX_JSON_FRAME_BYTES
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
    assert frames[0]["config"] == {"nested": {"value": "accepted"}}
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

    sdk.register_function("size::handler", handler, request_format=schema, response_format=schema, metadata=metadata)
    invocation = HttpInvocationConfig(url="https://example.invalid/handler", headers={"X-Test": "original"})
    sdk.register_function("size::http", invocation, request_format=schema, metadata=metadata)
    type_handler = TestTriggerHandler()
    sdk.register_trigger_type(
        {"id": "custom", "description": "accepted", "trigger_request_format": schema, "call_request_format": schema},
        type_handler,
    )
    schema["properties"]["value"]["type"] = "x" * _MAX_JSON_FRAME_BYTES
    metadata["nested"]["value"] = "mutated"
    invocation.headers["X-Test"] = "mutated"
    expected = {"type": "object", "properties": {"value": {"type": "string"}}}
    for registration in sdk._functions.values():
        assert registration.message.request_format == expected
        assert registration.message.metadata == {"nested": {"value": "original"}}
        sdk._prepare_json(registration.message)
    assert sdk._functions["size::handler"].message.response_format == expected
    assert sdk._functions["size::http"].message.invocation.headers == {"X-Test": "original"}
    assert sdk._trigger_types["custom"].message.trigger_request_format == expected
    assert sdk._trigger_types["custom"].message.call_request_format == expected
    assert sdk._trigger_types["custom"].handler is type_handler
    sdk._prepare_json(sdk._trigger_types["custom"].message)


def test_rejected_trigger_type_replacement_preserves_accepted_handler_and_message():
    sdk = registration_client()
    original_handler = TestTriggerHandler()
    sdk.register_trigger_type({"id": "custom", "description": "accepted"}, original_handler)
    retained = sdk._trigger_types["custom"]
    with pytest.raises(InvocationError, match="payload_too_large"):
        sdk.register_trigger_type({"id": "custom", "description": "x" * _MAX_JSON_FRAME_BYTES}, object())
    assert sdk._trigger_types["custom"] is retained
    assert retained.handler is original_handler
    assert sdk._queue == []
