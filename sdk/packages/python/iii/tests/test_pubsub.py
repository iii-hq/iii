"""Integration tests for PubSub operations."""

import asyncio
import random
import time

import pytest

from iii.iii import III


def unique_topic(prefix: str) -> str:
    return f"{prefix}.{int(time.time())}.{random.random():.10f}".replace(".", "_")


@pytest.mark.asyncio
async def test_subscribe_and_receive_published_messages(iii_client: III):
    """Subscribe to a topic and receive a published message."""
    topic = unique_topic("test_topic")
    received_messages = []
    received_event = asyncio.Event()

    async def subscriber_handler(data):
        received_messages.append(data)
        received_event.set()
        return {}

    fn = iii_client.register_function({"id": f"test.pubsub.py.subscriber.{topic}"}, subscriber_handler)
    trigger = iii_client.register_trigger({"type": "subscribe", "function_id": fn.id, "config": {"topic": topic}})

    await asyncio.sleep(0.3)

    try:
        iii_client.trigger(
            {
                "function_id": "publish",
                "payload": {"topic": topic, "data": {"message": "Hello PubSub!"}},
            }
        )

        await asyncio.wait_for(received_event.wait(), timeout=5.0)

        assert len(received_messages) == 1
        assert received_messages[0]["message"] == "Hello PubSub!"
    finally:
        fn.unregister()
        trigger.unregister()


@pytest.mark.asyncio
async def test_topic_isolation(iii_client: III):
    """Messages published to topic A do not reach topic B subscribers."""
    topic_a = unique_topic("topic_a")
    topic_b = unique_topic("topic_b")

    received_a = []
    received_b = []
    received_a_event = asyncio.Event()

    async def handler_a(data):
        received_a.append(data)
        received_a_event.set()
        return {}

    async def handler_b(data):
        received_b.append(data)
        return {}

    fn_a = iii_client.register_function({"id": f"test.pubsub.py.topic_a.{topic_a}"}, handler_a)
    fn_b = iii_client.register_function({"id": f"test.pubsub.py.topic_b.{topic_b}"}, handler_b)
    trigger_a = iii_client.register_trigger({"type": "subscribe", "function_id": fn_a.id, "config": {"topic": topic_a}})
    trigger_b = iii_client.register_trigger({"type": "subscribe", "function_id": fn_b.id, "config": {"topic": topic_b}})

    await asyncio.sleep(0.3)

    try:
        iii_client.trigger(
            {
                "function_id": "publish",
                "payload": {"topic": topic_a, "data": {"for": "a"}},
            }
        )

        await asyncio.wait_for(received_a_event.wait(), timeout=5.0)
        await asyncio.sleep(0.2)

        assert len(received_a) == 1
        assert len(received_b) == 0
    finally:
        fn_a.unregister()
        fn_b.unregister()
        trigger_a.unregister()
        trigger_b.unregister()
