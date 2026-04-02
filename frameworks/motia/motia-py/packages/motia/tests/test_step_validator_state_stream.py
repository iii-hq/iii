# motia/tests/test_step_validator_state_stream.py
"""Tests for step validation with state and stream triggers."""
import pytest

from motia.types import StepConfig, state, stream


def test_step_config_with_state_trigger_validates():
    """Test StepConfig accepts state trigger."""
    config = StepConfig(
        name="test-state-step",
        triggers=[state()],
    )
    assert config.triggers[0].type == "state"


def test_step_config_with_stream_trigger_validates():
    """Test StepConfig accepts stream trigger."""
    config = StepConfig(
        name="test-stream-step",
        triggers=[stream("todos")],
    )
    assert config.triggers[0].type == "stream"
    assert config.triggers[0].stream_name == "todos"


def test_step_config_with_mixed_triggers_validates():
    """Test StepConfig accepts mixed trigger types."""
    from motia import http, queue

    config = StepConfig(
        name="test-mixed-step",
        triggers=[
            queue("test.event"),
            http("GET", "/test"),
            state(),
            stream("todos"),
        ],
    )
    assert len(config.triggers) == 4
    assert config.triggers[0].type == "queue"
    assert config.triggers[1].type == "http"
    assert config.triggers[2].type == "state"
    assert config.triggers[3].type == "stream"


def test_stream_trigger_requires_stream_name():
    """Test StreamTrigger requires stream_name."""
    from motia.types import StreamTrigger

    with pytest.raises(Exception):  # ValidationError from Pydantic
        StreamTrigger(type="stream")  # Missing stream_name
