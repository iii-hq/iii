"""Compatibility tests for queue engine contract mapping."""

from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from motia.runtime import Motia
from motia.triggers import http, queue
from motia.types import ApiRequest, ApiResponse, FlowContext, QueueConfig, StepConfig


@pytest.fixture
def mock_bridge() -> MagicMock:
    """Create a bridge mock with call support."""
    bridge = MagicMock()
    bridge.register_function = MagicMock()
    bridge.register_trigger = MagicMock()
    bridge.call = MagicMock()
    return bridge


def test_queue_trigger_registers_as_queue_trigger(mock_bridge: MagicMock) -> None:
    """Queue trigger should map to queue trigger type for engine registration."""
    config = StepConfig(
        name="queue-trigger-compat",
        triggers=[queue("orders.created")],
    )

    def handler(input_data: object, ctx: FlowContext[object]) -> None:
        _ = (input_data, ctx)

    with (
        patch("motia.runtime.current_trace_id", return_value="test-trace-id"),
        patch("motia.runtime.get_instance", return_value=mock_bridge),
    ):
        motia = Motia()
        motia.add_step(config, "steps/test_step.py", handler)

    call_args = mock_bridge.register_trigger.call_args
    trigger_input = call_args[0][0]
    assert trigger_input["type"] == "queue"
    assert trigger_input["config"]["topic"] == "orders.created"


def test_standalone_enqueue_calls_bridge(mock_bridge: MagicMock) -> None:
    """Standalone enqueue() should call get_instance().trigger('enqueue', event)."""
    import sys

    _enqueue_mod = sys.modules["motia.enqueue"]

    with patch.object(_enqueue_mod, "get_instance", return_value=mock_bridge):
        from motia.enqueue import enqueue

        enqueue({"topic": "orders.processed", "data": {"order_id": "123"}})

    mock_bridge.trigger.assert_called_once_with(
        {"function_id": "enqueue", "payload": {"topic": "orders.processed", "data": {"order_id": "123"}}},
    )


@pytest.mark.asyncio
async def test_queue_handler_invoked_with_flow_context(mock_bridge: MagicMock) -> None:
    """Registered queue handler should pass input and FlowContext to the user handler."""
    captured_ctx = None
    captured_input = None

    config = StepConfig(
        name="queue-handler-ctx",
        triggers=[queue("orders.created")],
    )

    async def handler(input_data: object, ctx: FlowContext[object]) -> None:
        nonlocal captured_ctx, captured_input
        captured_ctx = ctx
        captured_input = input_data

    with (
        patch("motia.runtime.current_trace_id", return_value="test-trace-id"),
        patch("motia.runtime.get_instance", return_value=mock_bridge),
    ):
        motia = Motia()
        motia.add_step(config, "steps/test_step.py", handler)
        registered_handler = mock_bridge.register_function.call_args_list[0][0][1]
        await registered_handler({"source": "test"})

    assert captured_ctx is not None
    assert captured_ctx.trigger.type == "queue"
    assert captured_input == {"source": "test"}


@pytest.mark.asyncio
async def test_api_handler_invoked_with_flow_context(mock_bridge: MagicMock) -> None:
    """Registered API handler should pass MotiaHttpArgs and FlowContext to the user handler."""
    captured_ctx = None

    config = StepConfig(
        name="api-handler-ctx",
        triggers=[http("POST", "/orders")],
    )

    async def handler(req: object, ctx: FlowContext[dict]) -> ApiResponse[dict]:
        nonlocal captured_ctx
        captured_ctx = ctx
        return ApiResponse(status=200, body={"ok": True})

    with (
        patch("motia.runtime.current_trace_id", return_value="test-trace-id"),
        patch("motia.runtime.get_instance", return_value=mock_bridge),
    ):
        motia = Motia()
        motia.add_step(config, "steps/test_step.py", handler)
        registered_handler = mock_bridge.register_function.call_args_list[0][0][1]
        response_writer = MagicMock()
        response_writer.send_message_async = AsyncMock()
        response_writer.close = MagicMock()
        response_writer.stream = MagicMock()
        response_writer.stream.write = MagicMock()
        request_body_reader = MagicMock()
        await registered_handler(
            {
                "path": "/orders",
                "method": "POST",
                "path_params": {},
                "query_params": {},
                "body": {"hello": "world"},
                "headers": {},
                "response": response_writer,
                "request_body": request_body_reader,
            }
        )

    assert captured_ctx is not None
    assert captured_ctx.trigger.type == "http"


def test_queue_trigger_passes_queue_config(mock_bridge: MagicMock) -> None:
    """Queue trigger with config should include it as queue_config."""
    config = StepConfig(
        name="queue-config-test",
        triggers=[
            queue("orders", config=QueueConfig(max_retries=5, type="fifo")),
        ],
    )

    def handler(input_data: object, ctx: FlowContext[object]) -> None:
        _ = (input_data, ctx)

    with (
        patch("motia.runtime.current_trace_id", return_value="test-trace-id"),
        patch("motia.runtime.get_instance", return_value=mock_bridge),
    ):
        motia = Motia()
        motia.add_step(config, "steps/test_step.py", handler)

    call_args = mock_bridge.register_trigger.call_args
    trigger_config = call_args[0][0]["config"]
    assert trigger_config["queue_config"]["maxRetries"] == 5
    assert trigger_config["queue_config"]["type"] == "fifo"


def test_queue_trigger_omits_queue_config_when_not_provided(mock_bridge: MagicMock) -> None:
    """Queue trigger without config should not include queue_config."""
    config = StepConfig(
        name="queue-no-config-test",
        triggers=[queue("orders")],
    )

    def handler(input_data: object, ctx: FlowContext[object]) -> None:
        _ = (input_data, ctx)

    with (
        patch("motia.runtime.current_trace_id", return_value="test-trace-id"),
        patch("motia.runtime.get_instance", return_value=mock_bridge),
    ):
        motia = Motia()
        motia.add_step(config, "steps/test_step.py", handler)

    call_args = mock_bridge.register_trigger.call_args
    trigger_config = call_args[0][0]["config"]
    assert "queue_config" not in trigger_config
