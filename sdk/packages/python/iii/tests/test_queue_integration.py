"""Integration tests for queue enqueue and void trigger actions."""

import time

from iii import TriggerAction
from iii.iii import III


def wait_for(condition, timeout=5.0, interval=0.1):
    """Poll until condition() is truthy or timeout."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if condition():
            return
        time.sleep(interval)
    raise TimeoutError(f"Condition not met within {timeout}s")


def test_enqueue_delivers_message_to_function(iii_client: III):
    """Enqueue action delivers payload to the registered consumer function."""
    received = []

    def consumer_handler(input_data):
        received.append(input_data)
        return None

    ref = iii_client.register_function({"id": "test.queue.py.consumer"}, consumer_handler)
    time.sleep(0.3)

    try:

        result = iii_client.trigger({
            "function_id": "test.queue.py.consumer",
            "payload": {"order": "pizza"},
            "action": TriggerAction.Enqueue(queue="test-orders"),
        })

        assert isinstance(result, dict)
        assert isinstance(result["messageReceiptId"], str)

        wait_for(lambda: len(received) > 0, timeout=5.0)

        assert received[0]["order"] == "pizza"
    finally:
        ref.unregister()


def test_void_trigger_returns_none(iii_client: III):
    """Void trigger action fires without waiting and returns None."""
    calls = []

    def void_consumer_handler(input_data):
        calls.append(input_data)
        return None

    ref = iii_client.register_function({"id": "test.queue.py.void-consumer"}, void_consumer_handler)
    time.sleep(0.3)

    try:
        result = iii_client.trigger({
            "function_id": "test.queue.py.void-consumer",
            "payload": {"msg": "fire"},
            "action": TriggerAction.Void(),
        })

        assert result is None

        wait_for(lambda: len(calls) > 0, timeout=5.0)

        assert calls[0]["msg"] == "fire"
    finally:
        ref.unregister()


def test_enqueue_multiple_messages(iii_client: III):
    """Multiple enqueued messages are all delivered to the consumer."""
    received = []

    def multi_handler(input_data):
        received.append(input_data)
        return None

    ref = iii_client.register_function({"id": "test.queue.py.multi"}, multi_handler)
    time.sleep(0.3)

    try:
        for i in range(5):
            iii_client.trigger({
                "function_id": "test.queue.py.multi",
                "payload": {"index": i},
                "action": TriggerAction.Enqueue(queue="test-multi"),
            })

        wait_for(lambda: len(received) == 5, timeout=10.0)

        assert len(received) == 5
        received_indices = sorted(item["index"] for item in received)
        assert received_indices == [0, 1, 2, 3, 4]
    finally:
        ref.unregister()


def test_chained_enqueue(iii_client: III):
    """Function A enqueues a message to function B, forming a chain."""
    chain_received = []

    def chain_a_handler(input_data):
        iii_client.trigger({
            "function_id": "test.queue.py.chain-b",
            "payload": {**input_data, "chained": True},
            "action": TriggerAction.Enqueue(queue="test-chain"),
        })
        return input_data

    def chain_b_handler(input_data):
        chain_received.append(input_data)
        return None

    ref_a = iii_client.register_function({"id": "test.queue.py.chain-a"}, chain_a_handler)
    ref_b = iii_client.register_function({"id": "test.queue.py.chain-b"}, chain_b_handler)
    time.sleep(0.3)

    try:
        iii_client.trigger({
            "function_id": "test.queue.py.chain-a",
            "payload": {"origin": "test"},
            "action": TriggerAction.Enqueue(queue="test-chain"),
        })

        wait_for(lambda: len(chain_received) > 0, timeout=5.0)

        assert chain_received[0]["chained"] is True
        assert chain_received[0]["origin"] == "test"
    finally:
        ref_b.unregister()
        ref_a.unregister()
