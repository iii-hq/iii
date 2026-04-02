"""Tests for FlowContext state and stream methods."""

from unittest.mock import AsyncMock

import pytest

from motia.types import FlowContext, TriggerInfo


@pytest.fixture
def state_context():
    """Create FlowContext with state trigger."""
    return FlowContext(
        enqueue=AsyncMock(),
        trace_id="test-trace",
        state=None,
        logger=None,
        streams={},
        trigger=TriggerInfo(type="state", index=0),
        input_value={"group_id": "users", "item_id": "user-1"},
    )


@pytest.fixture
def stream_context():
    """Create FlowContext with stream trigger."""
    return FlowContext(
        enqueue=AsyncMock(),
        trace_id="test-trace",
        state=None,
        logger=None,
        streams={},
        trigger=TriggerInfo(type="stream", index=0),
        input_value={"stream_name": "todos", "event": {"type": "create"}},
    )


@pytest.fixture
def queue_context():
    """Create FlowContext with queue trigger."""
    return FlowContext(
        enqueue=AsyncMock(),
        trace_id="test-trace",
        state=None,
        logger=None,
        streams={},
        trigger=TriggerInfo(type="queue", topic="test.topic"),
        input_value={"topic": "test.topic", "data": {}},
    )


def test_is_state_returns_true_for_state_trigger(state_context):
    """Test is_state() returns True for state trigger."""
    assert state_context.is_state() is True
    assert state_context.is_stream() is False
    assert state_context.is_queue() is False


def test_is_stream_returns_true_for_stream_trigger(stream_context):
    """Test is_stream() returns True for stream trigger."""
    assert stream_context.is_stream() is True
    assert stream_context.is_state() is False
    assert stream_context.is_queue() is False


def test_is_state_returns_false_for_other_triggers(queue_context):
    """Test is_state() returns False for non-state triggers."""
    assert queue_context.is_state() is False


def test_is_stream_returns_false_for_other_triggers(queue_context):
    """Test is_stream() returns False for non-stream triggers."""
    assert queue_context.is_stream() is False


@pytest.mark.asyncio
async def test_match_calls_state_handler(state_context):
    """Test match() calls state handler for state trigger."""
    state_handler = AsyncMock(return_value="state_result")
    queue_handler = AsyncMock(return_value="queue_result")

    result = await state_context.match(
        {
            "state": state_handler,
            "queue": queue_handler,
        }
    )

    assert result == "state_result"
    state_handler.assert_called_once()
    queue_handler.assert_not_called()


@pytest.mark.asyncio
async def test_match_calls_stream_handler(stream_context):
    """Test match() calls stream handler for stream trigger."""
    stream_handler = AsyncMock(return_value="stream_result")
    queue_handler = AsyncMock(return_value="queue_result")

    result = await stream_context.match(
        {
            "stream": stream_handler,
            "queue": queue_handler,
        }
    )

    assert result == "stream_result"
    stream_handler.assert_called_once()
    queue_handler.assert_not_called()


@pytest.mark.asyncio
async def test_match_falls_back_to_default_for_state(state_context):
    """Test match() uses default handler when no state handler."""
    default_handler = AsyncMock(return_value="default_result")

    result = await state_context.match(
        {
            "queue": AsyncMock(),
            "default": default_handler,
        }
    )

    assert result == "default_result"
    default_handler.assert_called_once()
