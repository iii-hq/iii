# motia/tests/test_streams.py
"""Integration tests for Stream operations."""

import uuid
from unittest.mock import patch

import pytest

from motia import Stream

pytestmark = pytest.mark.integration


@pytest.fixture
def stream(bridge) -> Stream:
    """Create a test stream with unique name, using the test bridge."""
    stream_name = f"test_stream_{uuid.uuid4().hex[:8]}"
    with patch("motia.streams.get_instance", return_value=bridge):
        s = Stream(stream_name)
    return s


def _check_streams_available(stream: Stream) -> bool:
    """Check if streams functions are available in the engine.

    The streams.* functions may not be available in all engine configurations.
    Returns True if available, False otherwise.
    """
    try:
        # Try to call get on a non-existent item
        # If streams module is not available, this will raise an exception
        stream.get("__check__", "__check__")
        return True
    except Exception as e:
        if "function_not_found" in str(e):
            return False
        # Re-raise if it's a different error
        raise


def test_stream_set_and_get(bridge, stream):
    """Test setting and getting stream data."""
    with patch("motia.streams.get_instance", return_value=bridge):
        if not _check_streams_available(stream):
            pytest.skip("Streams module not available in engine configuration")

        group_id = f"group_{uuid.uuid4().hex[:8]}"
        item_id = f"item_{uuid.uuid4().hex[:8]}"
        data = {"name": "test", "value": 42}

        stream.set(group_id, item_id, data)

        result = stream.get(group_id, item_id)

        assert result is not None
        assert result["name"] == "test"
        assert result["value"] == 42


def test_stream_delete(bridge, stream):
    """Test deleting stream data."""
    with patch("motia.streams.get_instance", return_value=bridge):
        if not _check_streams_available(stream):
            pytest.skip("Streams module not available in engine configuration")

        group_id = f"group_delete_{uuid.uuid4().hex[:8]}"
        item_id = f"item_delete_{uuid.uuid4().hex[:8]}"

        stream.set(group_id, item_id, {"temp": True})

        # Verify exists
        result = stream.get(group_id, item_id)
        assert result is not None

        # Delete
        stream.delete(group_id, item_id)

        # Verify deleted
        result = stream.get(group_id, item_id)
        assert result is None


def test_stream_get_group(bridge, stream):
    """Test getting all items in a group."""
    with patch("motia.streams.get_instance", return_value=bridge):
        if not _check_streams_available(stream):
            pytest.skip("Streams module not available in engine configuration")

        group_id = f"group_list_{uuid.uuid4().hex[:8]}"

        # Add items
        for i in range(3):
            stream.set(group_id, f"item_{i}", {"index": i})

        # Get group
        items = stream.get_group(group_id)

        assert len(items) == 3


def test_stream_get_nonexistent(bridge, stream):
    """Test getting non-existent data returns None."""
    with patch("motia.streams.get_instance", return_value=bridge):
        if not _check_streams_available(stream):
            pytest.skip("Streams module not available in engine configuration")

        result = stream.get("nonexistent_group", "nonexistent_item")
        assert result is None
