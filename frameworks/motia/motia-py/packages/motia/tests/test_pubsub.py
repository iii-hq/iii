# motia/tests/test_pubsub.py
"""Integration tests for PubSub operations."""

import threading
import time
import uuid

import pytest

from tests.conftest import flush_bridge_queue

pytestmark = pytest.mark.integration


def test_pubsub_subscribe_and_publish(bridge):
    """Test subscribing to a topic and receiving published messages."""
    topic = f"test.topic.{uuid.uuid4().hex[:8]}"
    received_messages = []
    message_received = threading.Event()

    def subscriber_handler(data):
        received_messages.append(data)
        message_received.set()
        return {}

    # Register subscriber function
    bridge.register_function({"id": f"test.pubsub.subscriber.{topic}"}, subscriber_handler)

    # Register subscribe trigger
    bridge.register_trigger(
        {
            "type": "subscribe",
            "function_id": f"test.pubsub.subscriber.{topic}",
            "config": {
                "topic": topic,
            },
        }
    )

    flush_bridge_queue(bridge)
    time.sleep(0.3)

    # Publish a message
    bridge.trigger(
        {
            "function_id": "publish",
            "payload": {
                "topic": topic,
                "data": {"message": "Hello PubSub!"},
            },
        }
    )

    # Wait for message
    if not message_received.wait(timeout=5.0):
        raise TimeoutError("Did not receive message within 5s")

    assert len(received_messages) == 1
    # Data may have _caller_worker_id injected
    assert received_messages[0].get("message") == "Hello PubSub!"


def test_pubsub_different_topics(bridge):
    """Test that subscribers only receive messages from their topic."""
    topic_a = f"topic.a.{uuid.uuid4().hex[:8]}"
    topic_b = f"topic.b.{uuid.uuid4().hex[:8]}"

    received_a = []
    received_b = []
    message_a_received = threading.Event()

    def subscriber_a(data):
        received_a.append(data)
        message_a_received.set()
        return {}

    def subscriber_b(data):
        received_b.append(data)
        return {}

    bridge.register_function({"id": f"test.pubsub.topic_a.{topic_a}"}, subscriber_a)
    bridge.register_function({"id": f"test.pubsub.topic_b.{topic_b}"}, subscriber_b)

    bridge.register_trigger(
        {"type": "subscribe", "function_id": f"test.pubsub.topic_a.{topic_a}", "config": {"topic": topic_a}}
    )
    bridge.register_trigger(
        {"type": "subscribe", "function_id": f"test.pubsub.topic_b.{topic_b}", "config": {"topic": topic_b}}
    )

    flush_bridge_queue(bridge)
    time.sleep(0.3)

    # Publish to topic A only
    bridge.trigger(
        {
            "function_id": "publish",
            "payload": {
                "topic": topic_a,
                "data": {"for": "a"},
            },
        }
    )

    if not message_a_received.wait(timeout=5.0):
        raise TimeoutError("Did not receive message within 5s")

    # Give time for any erroneous messages to arrive
    time.sleep(0.2)

    assert len(received_a) == 1
    assert len(received_b) == 0
