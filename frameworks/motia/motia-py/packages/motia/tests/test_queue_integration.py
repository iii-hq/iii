"""Integration tests for queue enqueue and subscribe."""

import time

import pytest

from tests.conftest import flush_bridge_queue, wait_for_registration

pytestmark = pytest.mark.integration


def test_enqueue_delivers_message_to_subscribed_handler(bridge):
    """Enqueue delivers message to subscribed handler."""
    function_id = f"test.queue.basic.{int(time.time() * 1000)}"
    topic = f"test-topic-{int(time.time() * 1000)}"
    received = []

    def handler(data):
        received.append(data)

    bridge.register_function({"id": function_id}, handler)
    bridge.register_trigger({"type": "queue", "function_id": function_id, "config": {"topic": topic}})

    flush_bridge_queue(bridge)
    time.sleep(0.5)

    bridge.trigger({"function_id": "enqueue", "payload": {"topic": topic, "data": {"order": "abc"}}})
    time.sleep(1.5)

    assert received == [{"order": "abc"}]


def test_handler_receives_exact_data_payload_from_enqueue(bridge):
    """Handler receives exact data payload from enqueue."""
    function_id = f"test.queue.payload.{int(time.time() * 1000)}"
    topic = f"test-topic-payload-{int(time.time() * 1000)}"
    payload = {"id": "x1", "count": 42, "nested": {"a": 1}}
    received = []

    def handler(data):
        received.append(data)

    bridge.register_function({"id": function_id}, handler)
    bridge.register_trigger({"type": "queue", "function_id": function_id, "config": {"topic": topic}})

    flush_bridge_queue(bridge)
    time.sleep(0.5)

    bridge.trigger({"function_id": "enqueue", "payload": {"topic": topic, "data": payload}})
    time.sleep(1.5)

    assert received == [payload]


def test_subscription_with_queue_config_receives_messages(bridge):
    """Subscription with queue config receives messages."""
    function_id = f"test.queue.infra.{int(time.time() * 1000)}"
    topic = f"test-topic-infra-{int(time.time() * 1000)}"
    received = []

    def handler(data):
        received.append(data)

    bridge.register_function({"id": function_id}, handler)
    bridge.register_trigger(
        {
            "type": "queue",
            "function_id": function_id,
            "config": {
                "topic": topic,
                "queue_config": {
                    "maxRetries": 5,
                    "type": "standard",
                    "concurrency": 2,
                },
            },
        }
    )

    flush_bridge_queue(bridge)
    time.sleep(0.5)

    bridge.trigger({"function_id": "enqueue", "payload": {"topic": topic, "data": {"infra": True}}})
    time.sleep(1.5)

    assert received == [{"infra": True}]


def test_multiple_subscribers_on_same_topic_messages_delivered_to_one(bridge):
    """Multiple subscribers on same topic - messages delivered to exactly one subscriber."""
    topic = f"test-topic-multi-{int(time.time() * 1000)}"
    function_id1 = f"test.queue.multi1.{int(time.time() * 1000)}"
    function_id2 = f"test.queue.multi2.{int(time.time() * 1000)}"
    received1 = []
    received2 = []

    def handler1(data):
        received1.append(data)

    def handler2(data):
        received2.append(data)

    bridge.register_function({"id": function_id1}, handler1)
    bridge.register_function({"id": function_id2}, handler2)
    bridge.register_trigger({"type": "queue", "function_id": function_id1, "config": {"topic": topic}})
    bridge.register_trigger({"type": "queue", "function_id": function_id2, "config": {"topic": topic}})

    flush_bridge_queue(bridge)
    time.sleep(0.5)

    bridge.trigger({"function_id": "enqueue", "payload": {"topic": topic, "data": {"msg": 1}}})
    bridge.trigger({"function_id": "enqueue", "payload": {"topic": topic, "data": {"msg": 2}}})
    time.sleep(2.0)

    total = len(received1) + len(received2)
    assert total == 2
    all_received = received1 + received2
    assert {"msg": 1} in all_received
    assert {"msg": 2} in all_received


def test_condition_function_filters_messages(bridge):
    """Condition function filters messages."""
    topic = f"test-topic-cond-{int(time.time() * 1000)}"
    function_id = f"test.queue.cond.{int(time.time() * 1000)}"
    condition_path = f"{function_id}::conditions::0"
    handler_calls = []

    def handler(data):
        handler_calls.append(data)

    def condition(input_data):
        return input_data.get("accept") is True

    bridge.register_function({"id": function_id}, handler)
    bridge.register_function({"id": condition_path}, condition)
    bridge.register_trigger(
        {
            "type": "queue",
            "function_id": function_id,
            "config": {
                "topic": topic,
                "condition_function_id": condition_path,
            },
        }
    )

    flush_bridge_queue(bridge)
    wait_for_registration(bridge, function_id)
    wait_for_registration(bridge, condition_path)

    bridge.trigger({"function_id": "enqueue", "payload": {"topic": topic, "data": {"accept": False}}})
    bridge.trigger({"function_id": "enqueue", "payload": {"topic": topic, "data": {"accept": True}}})
    time.sleep(2.0)

    assert len(handler_calls) == 1
    assert handler_calls[0] == {"accept": True}
