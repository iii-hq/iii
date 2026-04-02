# motia/tests/test_bridge.py
"""Integration tests for bridge connection and function registration."""

import time
import threading

import pytest
from iii import TriggerAction

from tests.conftest import flush_bridge_queue

pytestmark = pytest.mark.integration


def test_bridge_connects_successfully(bridge):
    """Test that bridge connects to III engine."""
    # If we get here, the bridge fixture connected successfully
    assert bridge is not None
    assert bridge._ws is not None


def test_register_and_invoke_function(bridge):
    """Test registering a function and invoking it."""
    received_data = None

    def echo_handler(data):
        nonlocal received_data
        received_data = data
        return {"echoed": data}

    bridge.register_function({"id": "test.echo"}, echo_handler)

    flush_bridge_queue(bridge)

    # Give time for registration to propagate
    time.sleep(0.3)

    # Invoke the function
    result = bridge.trigger({"function_id": "test.echo", "payload": {"message": "hello"}})

    # Check that the echoed data contains our message
    # (III engine may inject _caller_worker_id into the data)
    assert "echoed" in result
    assert result["echoed"]["message"] == "hello"
    assert received_data["message"] == "hello"


def test_invoke_function_async_fire_and_forget(bridge):
    """Test fire-and-forget function invocation."""
    received = threading.Event()
    received_data = None

    def receiver(data):
        nonlocal received_data
        received_data = data
        received.set()
        return {}

    bridge.register_function({"id": "test.receiver"}, receiver)

    flush_bridge_queue(bridge)

    time.sleep(0.3)

    # Fire and forget
    bridge.trigger({"function_id": "test.receiver", "payload": {"value": 42}, "action": TriggerAction.Void()})

    # Wait for it to be received
    if not received.wait(timeout=5.0):
        raise TimeoutError("Did not receive message within 5s")

    # Check that the received data contains our value
    # (III engine may inject _caller_worker_id into the data)
    assert received_data["value"] == 42


def test_invoke_nonexistent_function_raises(bridge):
    """Test that invoking a non-existent function raises an error."""
    with pytest.raises(Exception):
        bridge.trigger({"function_id": "nonexistent.function", "payload": {}, "timeout": 2.0})
