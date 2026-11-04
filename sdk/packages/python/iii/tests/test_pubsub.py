"""Integration tests for PubSub: subscribers observe ``publish`` with a
filter on ``input.topic`` and receive the published input."""

import asyncio
import random
import time

import pytest

from iii import ObserverHandle
from iii.iii import III

# An observer of a function nobody hosts stays pending; bound the wait so a
# missing pubsub worker fails the test instead of hanging it.
READY_TIMEOUT_S = 10.0


def unique_topic(prefix: str) -> str:
    return f"{prefix}.{int(time.time())}.{random.random():.10f}".replace(".", "_")


def subscribe(iii_client: III, topic: str, function_id: str) -> ObserverHandle:
    """A subscriber: every publish to ``topic`` calls ``function_id`` with the published input."""
    return iii_client.observe(
        {"function_id": "publish", "filters": {"input.topic": {"$eq": topic}}, "deliver": "input"},
        function_id,
    )


@pytest.mark.asyncio
async def test_subscribe_and_receive_published_messages(iii_client: III):
    """Subscribe to a topic and receive a published message."""
    topic = unique_topic("test_topic")
    received_messages = []
    received_event = asyncio.Event()

    async def subscriber_handler(published):
        received_messages.append(published["data"])
        received_event.set()
        return {}

    fn = iii_client.register_function(f"test.pubsub.py.subscriber.{topic}", subscriber_handler)
    subscription = subscribe(iii_client, topic, fn.id)
    await asyncio.wait_for(subscription.ready_async(), timeout=READY_TIMEOUT_S)

    try:
        iii_client.call(
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
        subscription.dispose()


@pytest.mark.asyncio
async def test_topic_isolation(iii_client: III):
    """Messages published to topic A do not reach topic B subscribers."""
    topic_a = unique_topic("topic_a")
    topic_b = unique_topic("topic_b")

    received_a = []
    received_b = []
    received_a_event = asyncio.Event()

    async def handler_a(published):
        received_a.append(published["data"])
        received_a_event.set()
        return {}

    async def handler_b(published):
        received_b.append(published["data"])
        return {}

    fn_a = iii_client.register_function(f"test.pubsub.py.topic_a.{topic_a}", handler_a)
    fn_b = iii_client.register_function(f"test.pubsub.py.topic_b.{topic_b}", handler_b)
    subscription_a = subscribe(iii_client, topic_a, fn_a.id)
    subscription_b = subscribe(iii_client, topic_b, fn_b.id)
    await asyncio.wait_for(subscription_a.ready_async(), timeout=READY_TIMEOUT_S)
    await asyncio.wait_for(subscription_b.ready_async(), timeout=READY_TIMEOUT_S)

    try:
        iii_client.call(
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
        subscription_a.dispose()
        subscription_b.dispose()
