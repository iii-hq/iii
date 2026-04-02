# motia/tests/test_state_trigger_types.py
"""Tests for State trigger types."""
import pytest
from motia.types import (
    StateTrigger,
    StateTriggerInput,
    state,
)


def test_state_trigger_input_model():
    """Test StateTriggerInput accepts required fields."""
    trigger_input = StateTriggerInput(
        type="state",
        group_id="users",
        item_id="user-123",
        old_value={"name": "Alice"},
        new_value={"name": "Bob"},
    )
    assert trigger_input.type == "state"
    assert trigger_input.group_id == "users"
    assert trigger_input.item_id == "user-123"
    assert trigger_input.old_value == {"name": "Alice"}
    assert trigger_input.new_value == {"name": "Bob"}


def test_state_trigger_input_optional_values():
    """Test StateTriggerInput with optional old/new values."""
    trigger_input = StateTriggerInput(
        type="state",
        group_id="orders",
        item_id="order-456",
    )
    assert trigger_input.old_value is None
    assert trigger_input.new_value is None


def test_state_trigger_model():
    """Test StateTrigger model structure."""
    trigger = StateTrigger(type="state")
    assert trigger.type == "state"
    assert trigger.condition is None


def test_state_trigger_with_condition():
    """Test StateTrigger with condition function."""
    def my_condition(input, ctx):
        return input.group_id == "users"

    trigger = StateTrigger(type="state", condition=my_condition)
    assert trigger.type == "state"
    assert trigger.condition is my_condition


def test_state_helper_function():
    """Test state() helper creates StateTrigger."""
    trigger = state()
    assert isinstance(trigger, StateTrigger)
    assert trigger.type == "state"
    assert trigger.condition is None


def test_state_helper_with_condition():
    """Test state() helper with condition."""
    def check_value(input, ctx):
        return input.new_value is not None

    trigger = state(condition=check_value)
    assert trigger.condition is check_value


def test_state_trigger_input_serialization():
    """Test StateTriggerInput serializes with correct aliases."""
    trigger_input = StateTriggerInput(
        group_id="users",
        item_id="user-123",
        old_value={"name": "Alice"},
        new_value={"name": "Bob"},
    )
    data = trigger_input.model_dump(by_alias=True, exclude_none=True)
    assert data["groupId"] == "users"
    assert data["itemId"] == "user-123"
    assert data["oldValue"] == {"name": "Alice"}
    assert data["newValue"] == {"name": "Bob"}


def test_state_trigger_input_from_camel_case():
    """Test StateTriggerInput can be created from camelCase JSON."""
    trigger_input = StateTriggerInput(
        type="state",
        groupId="users",
        itemId="user-123",
        oldValue={"name": "Alice"},
        newValue={"name": "Bob"},
    )
    assert trigger_input.group_id == "users"
    assert trigger_input.item_id == "user-123"
    assert trigger_input.old_value == {"name": "Alice"}
    assert trigger_input.new_value == {"name": "Bob"}
