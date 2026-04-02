"""Tests for TriggerInfo with state and stream types."""
from motia.types import TriggerInfo


def test_trigger_info_state_type():
    """Test TriggerInfo supports state type."""
    meta = TriggerInfo(type="state", index=0)
    assert meta.type == "state"
    assert meta.index == 0


def test_trigger_info_queue_type():
    """Test TriggerInfo supports queue type with topic."""
    meta = TriggerInfo(
        type="queue",
        index=0,
        topic="test.topic",
    )
    assert meta.type == "queue"
    assert meta.topic == "test.topic"


def test_trigger_info_stream_type():
    """Test TriggerInfo supports stream type."""
    meta = TriggerInfo(
        type="stream",
        index=0,
    )
    assert meta.type == "stream"
    assert meta.index == 0


def test_trigger_info_minimal():
    """Test TriggerInfo with only required fields."""
    meta = TriggerInfo(type="stream")
    assert meta.type == "stream"
    assert meta.index is None
    assert meta.topic is None
