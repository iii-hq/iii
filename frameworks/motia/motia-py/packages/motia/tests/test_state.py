# motia/tests/test_state.py
"""Integration tests for StateManager."""

import uuid
from unittest.mock import patch

import pytest

from motia.state import StateManager

pytestmark = pytest.mark.integration


@pytest.fixture
def state_manager(bridge) -> StateManager:
    """Create a test state manager with the test bridge."""
    return StateManager()


def _check_state_available(state_manager, bridge):
    """Check if state functions are available in the engine."""
    try:
        with patch("motia.state.get_instance", return_value=bridge):
            state_manager.get(f"test_{uuid.uuid4().hex}", "check")
        return True
    except Exception as e:
        if "function_not_found" in str(e).lower():
            return False
        # Other errors might indicate the module exists but had an issue
        return True


def test_state_set_and_get(state_manager, bridge):
    """Test setting and getting state."""
    with patch("motia.state.get_instance", return_value=bridge):
        if not _check_state_available(state_manager, bridge):
            pytest.skip("State module not available in engine configuration")

        scope = f"test_scope_{uuid.uuid4().hex[:8]}"
        key = f"test_key_{uuid.uuid4().hex[:8]}"
        value = {"status": "active", "count": 10}

        state_manager.set(scope, key, value)

        result = state_manager.get(scope, key)

        assert result is not None
        assert result["status"] == "active"
        assert result["count"] == 10


def test_state_delete(state_manager, bridge):
    """Test deleting state."""
    with patch("motia.state.get_instance", return_value=bridge):
        if not _check_state_available(state_manager, bridge):
            pytest.skip("State module not available in engine configuration")

        scope = f"delete_scope_{uuid.uuid4().hex[:8]}"
        key = f"delete_key_{uuid.uuid4().hex[:8]}"

        state_manager.set(scope, key, {"temp": True})

        # Verify exists
        result = state_manager.get(scope, key)
        assert result is not None

        # Delete
        state_manager.delete(scope, key)

        # Verify deleted
        result = state_manager.get(scope, key)
        assert result is None


def test_state_get_nonexistent(state_manager, bridge):
    """Test getting non-existent state returns None."""
    with patch("motia.state.get_instance", return_value=bridge):
        if not _check_state_available(state_manager, bridge):
            pytest.skip("State module not available in engine configuration")

        result = state_manager.get(f"nonexistent_{uuid.uuid4().hex}", "nonexistent")
        assert result is None


def test_state_list(state_manager, bridge):
    """Test listing all items in a state scope."""
    with patch("motia.state.get_instance", return_value=bridge):
        if not _check_state_available(state_manager, bridge):
            pytest.skip("State module not available in engine configuration")

        scope = f"list_scope_{uuid.uuid4().hex[:8]}"

        # Add items
        for i in range(3):
            state_manager.set(scope, f"item_{i}", {"index": i})

        # List
        items = state_manager.list(scope)

        assert len(items) == 3
