# motia/tests/test_stream_trigger_types.py
"""Tests for Stream trigger types."""
import pytest
from motia.types import (
    StreamTrigger,
    StreamTriggerInput,
    StreamEvent,
    stream,
)


def test_stream_event_create():
    """Test StreamEvent with create type."""
    event = StreamEvent(type="create", data={"id": "123", "name": "Test"})
    assert event.type == "create"
    assert event.data == {"id": "123", "name": "Test"}


def test_stream_event_update():
    """Test StreamEvent with update type."""
    event = StreamEvent(type="update", data={"id": "123", "name": "Updated"})
    assert event.type == "update"


def test_stream_event_delete():
    """Test StreamEvent with delete type."""
    event = StreamEvent(type="delete", data={"id": "123"})
    assert event.type == "delete"


def test_stream_trigger_input_model():
    """Test StreamTriggerInput accepts required fields."""
    event = StreamEvent(type="create", data={"name": "Test"})
    trigger_input = StreamTriggerInput(
        type="stream",
        timestamp=1234567890,
        stream_name="todos",
        group_id="inbox",
        id="item-123",
        event=event,
    )
    assert trigger_input.type == "stream"
    assert trigger_input.timestamp == 1234567890
    assert trigger_input.stream_name == "todos"
    assert trigger_input.group_id == "inbox"
    assert trigger_input.id == "item-123"
    assert trigger_input.event.type == "create"


def test_stream_trigger_model():
    """Test StreamTrigger model structure."""
    trigger = StreamTrigger(type="stream", stream_name="todos")
    assert trigger.type == "stream"
    assert trigger.stream_name == "todos"
    assert trigger.group_id is None
    assert trigger.item_id is None
    assert trigger.condition is None


def test_stream_trigger_with_filters():
    """Test StreamTrigger with optional filters."""
    trigger = StreamTrigger(
        type="stream",
        stream_name="orders",
        group_id="pending",
        item_id="order-123",
    )
    assert trigger.stream_name == "orders"
    assert trigger.group_id == "pending"
    assert trigger.item_id == "order-123"


def test_stream_trigger_with_condition():
    """Test StreamTrigger with condition function."""
    def my_condition(input, ctx):
        return input.event.type == "update"

    trigger = StreamTrigger(
        type="stream",
        stream_name="todos",
        condition=my_condition,
    )
    assert trigger.condition is my_condition


def test_stream_helper_function():
    """Test stream() helper creates StreamTrigger."""
    trigger = stream("todos")
    assert isinstance(trigger, StreamTrigger)
    assert trigger.type == "stream"
    assert trigger.stream_name == "todos"


def test_stream_helper_with_all_options():
    """Test stream() helper with all options."""
    def check_update(input, ctx):
        return input.event.type == "update"

    trigger = stream(
        "orders",
        group_id="pending",
        item_id="order-123",
        condition=check_update,
    )
    assert trigger.stream_name == "orders"
    assert trigger.group_id == "pending"
    assert trigger.item_id == "order-123"
    assert trigger.condition is check_update


def test_stream_trigger_input_serialization():
    """Test StreamTriggerInput serializes with correct aliases."""
    event = StreamEvent(type="create", data={"name": "Test"})
    trigger_input = StreamTriggerInput(
        timestamp=1234567890,
        stream_name="todos",
        group_id="inbox",
        id="item-123",
        event=event,
    )
    data = trigger_input.model_dump(by_alias=True, exclude_none=True)
    assert data["streamName"] == "todos"
    assert data["groupId"] == "inbox"


def test_stream_trigger_input_from_camel_case():
    """Test StreamTriggerInput can be created from camelCase JSON."""
    trigger_input = StreamTriggerInput(
        type="stream",
        timestamp=1234567890,
        streamName="todos",
        groupId="inbox",
        id="item-123",
        event={"type": "create", "data": {"name": "Test"}},
    )
    assert trigger_input.stream_name == "todos"
    assert trigger_input.group_id == "inbox"
