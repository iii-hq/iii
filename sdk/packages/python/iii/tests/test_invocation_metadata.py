"""Tests for per-invocation metadata support.

Covers the wire mirror (``InvokeFunctionMessage.metadata``), the handler
signature convention (metadata is forwarded only when the handler accepts
it -- 1-arg handlers keep working), and the outbound ``trigger`` API.
"""

from unittest.mock import AsyncMock, patch

import pytest

import iii.iii as iii_module
from iii.iii import III, _metadata_passing_mode
from iii.iii_constants import InitOptions
from iii.iii_types import InvokeFunctionMessage, TriggerActionVoid, TriggerRequest


@pytest.fixture
def client(monkeypatch):
    """A client whose background loop runs but never connects to an engine.

    Constructing an ``III`` runs ``connect_async`` -> ``init_otel``, which
    installs global OTel providers. We reset those singletons on teardown so
    this test does not leak provider state into later tests (mirrors the
    cleanup in ``test_context_propagation.py``).
    """

    async def fake_do_connect(self):
        return None

    monkeypatch.setattr(iii_module.III, "_do_connect", fake_do_connect)
    c = III(address="ws://localhost:9999", options=InitOptions(worker_name="test"))
    yield c
    c.shutdown()
    _reset_otel_singletons()


def _reset_otel_singletons():
    """Reset OTel global provider singletons so tests stay isolated."""
    try:
        import opentelemetry._logs._internal as _li

        _li._LOGGER_PROVIDER = None
        _li._LOGGER_PROVIDER_SET_ONCE._done = False
    except Exception:
        pass
    try:
        import opentelemetry.trace._internal as _ti

        _ti._TRACER_PROVIDER = None
        _ti._TRACER_PROVIDER_SET_ONCE._done = False
    except Exception:
        pass
    try:
        import opentelemetry.metrics._internal as _mi

        _mi._METER_PROVIDER = None
        _mi._METER_PROVIDER_SET_ONCE._done = False
    except Exception:
        pass


# --- _metadata_passing_mode: handler-arity detection -----------------------


def test_mode_positional_for_two_arg_handler():
    def handler(data, metadata=None):
        return None

    assert _metadata_passing_mode(handler) == "positional"


def test_mode_positional_for_required_metadata_param():
    def handler(data, metadata):
        return None

    assert _metadata_passing_mode(handler) == "positional"


def test_mode_keyword_for_keyword_only_metadata():
    def handler(data, *, metadata=None):
        return None

    assert _metadata_passing_mode(handler) == "keyword"


def test_mode_none_for_single_arg_handler():
    def handler(data):
        return None

    assert _metadata_passing_mode(handler) == "none"


def test_mode_none_for_unrelated_second_positional_param():
    # A pre-existing handler with an unrelated optional second parameter must
    # keep its exact old call shape: handler(data), so `retries` stays 3.
    def handler(data, retries=3):
        return None

    assert _metadata_passing_mode(handler) == "none"


def test_mode_none_for_var_keyword_handler():
    # **kwargs handlers previously received no keyword arguments; injecting
    # metadata into kwargs would change their observable input.
    def handler(data, **kwargs):
        return None

    assert _metadata_passing_mode(handler) == "none"


def test_mode_none_for_var_positional_handler():
    # *args handlers previously received exactly one argument; appending
    # metadata would change len(args).
    def handler(*args):
        return None

    assert _metadata_passing_mode(handler) == "none"


# --- handler receives invocation metadata when it accepts it ---------------


def test_sync_handler_receives_invocation_metadata(client):
    received = {}

    def handler(data, metadata=None):
        received["data"] = data
        received["metadata"] = metadata
        return {"ok": True}

    client.register_function("fn::meta", handler)

    with patch.object(client, "_send", new_callable=AsyncMock):
        client._run_on_loop(
            client._handle_invoke(
                invocation_id="inv-1",
                path="fn::meta",
                data={"x": 1},
                metadata={"tenant": "acme"},
            )
        )

    assert received["data"] == {"x": 1}
    assert received["metadata"] == {"tenant": "acme"}


def test_async_handler_receives_invocation_metadata(client):
    received = {}

    async def handler(data, metadata=None):
        received["metadata"] = metadata
        return {"ok": True}

    client.register_function("fn::async-meta", handler)

    with patch.object(client, "_send", new_callable=AsyncMock):
        client._run_on_loop(
            client._handle_invoke(
                invocation_id="inv-2",
                path="fn::async-meta",
                data={},
                metadata={"region": "us"},
            )
        )

    assert received["metadata"] == {"region": "us"}


def test_keyword_only_handler_receives_invocation_metadata(client):
    received = {}

    def handler(data, *, metadata=None):
        received["metadata"] = metadata
        return {"ok": True}

    client.register_function("fn::kw-meta", handler)

    with patch.object(client, "_send", new_callable=AsyncMock):
        client._run_on_loop(
            client._handle_invoke(
                invocation_id="inv-3",
                path="fn::kw-meta",
                data={},
                metadata={"k": "v"},
            )
        )

    assert received["metadata"] == {"k": "v"}


# --- back-compat: 1-arg handler keeps working when metadata is present -----


def test_one_arg_handler_still_works_with_metadata_present(client):
    received = {}

    def handler(data):
        received["data"] = data
        return {"ok": True}

    client.register_function("fn::legacy", handler)

    with patch.object(client, "_send", new_callable=AsyncMock) as send_mock:
        client._run_on_loop(
            client._handle_invoke(
                invocation_id="inv-4",
                path="fn::legacy",
                data={"x": 2},
                metadata={"present": True},
            )
        )

    # Handler ran with just `data` and did not crash.
    assert received["data"] == {"x": 2}
    # The invocation completed successfully (a result, not an error, was sent).
    result_msg = send_mock.call_args.args[0]
    assert result_msg.error is None
    assert result_msg.result == {"ok": True}


# --- outbound trigger() populates the wire metadata field ------------------


def test_trigger_async_dict_populates_outbound_metadata(client):
    with patch.object(client, "_send", new_callable=AsyncMock) as send_mock:
        client._run_on_loop(
            client.trigger_async(
                {
                    "function_id": "remote::fn",
                    "payload": {"a": 1},
                    "action": TriggerActionVoid(),
                    "metadata": {"caller": "svc-a"},
                }
            )
        )

    msg = send_mock.call_args.args[0]
    assert isinstance(msg, InvokeFunctionMessage)
    assert msg.data == {"a": 1}
    assert msg.metadata == {"caller": "svc-a"}


def test_trigger_async_request_object_populates_outbound_metadata(client):
    with patch.object(client, "_send", new_callable=AsyncMock) as send_mock:
        client._run_on_loop(
            client.trigger_async(
                TriggerRequest(
                    function_id="remote::fn",
                    payload={"a": 1},
                    action=TriggerActionVoid(),
                    metadata={"caller": "svc-b"},
                )
            )
        )

    msg = send_mock.call_args.args[0]
    assert msg.metadata == {"caller": "svc-b"}


def test_trigger_async_without_metadata_leaves_field_none(client):
    with patch.object(client, "_send", new_callable=AsyncMock) as send_mock:
        client._run_on_loop(
            client.trigger_async(
                {
                    "function_id": "remote::fn",
                    "payload": {},
                    "action": TriggerActionVoid(),
                }
            )
        )

    msg = send_mock.call_args.args[0]
    assert msg.metadata is None


# --- InvokeFunctionMessage round-trip / omission ---------------------------


def test_invoke_function_message_roundtrips_metadata():
    msg = InvokeFunctionMessage(function_id="fn", data={"x": 1}, metadata={"trace": "abc"})
    dumped = msg.model_dump(by_alias=True, exclude_none=True)
    assert dumped["metadata"] == {"trace": "abc"}

    parsed = InvokeFunctionMessage.model_validate(dumped)
    assert parsed.metadata == {"trace": "abc"}


def test_invoke_function_message_omits_metadata_when_none():
    msg = InvokeFunctionMessage(function_id="fn", data={"x": 1})
    dumped = msg.model_dump(by_alias=True, exclude_none=True)
    assert "metadata" not in dumped


def test_invoke_function_message_absent_metadata_parses_to_none():
    # Backward compatible: an inbound message with no `metadata` key.
    parsed = InvokeFunctionMessage.model_validate({"function_id": "fn", "data": {}})
    assert parsed.metadata is None
