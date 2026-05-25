"""
Pattern: Custom Triggers
Comparable to: Custom event adapters, webhook connectors, subscription bridges

Demonstrates how to define entirely new trigger types beyond the built-in
http, durable:subscriber, cron, state, and subscribe triggers. A custom trigger type
registers handler callbacks that the engine invokes when triggers of that
type are created or removed, letting you bridge any external event source
(webhooks, message subscriptions) into the iii function graph.

Note: File watcher is omitted — it requires the watchdog dependency.

How-to references:
  - Custom trigger types: https://iii.dev/docs/how-to/create-custom-trigger-type
"""

import asyncio
import os
from collections import defaultdict

from iii import InitOptions, Logger, TriggerHandler, register_worker

iii = register_worker(
    address=os.environ.get("III_ENGINE_URL", "ws://localhost:49134"),
    options=InitOptions(worker_name="custom-triggers"),
)

# ---
# Custom trigger type — Webhook receiver
# Registers an HTTP endpoint per trigger and fires the bound function when
# an external service POSTs to it.
# ---
webhook_endpoints = {}


async def webhook_register(trigger_config):
    logger = Logger()
    trigger_id = trigger_config["id"]
    function_id = trigger_config["function_id"]
    config = trigger_config["config"]
    path = config.get("path", f"/webhooks/{trigger_id}")

    logger.info("Registering webhook endpoint", {"id": trigger_id, "path": path})

    async def callback(request_body):
        await iii.trigger_async({
            "function_id": function_id,
            "payload": {"source": "webhook", "trigger_id": trigger_id, "data": request_body},
        })

    webhook_endpoints[trigger_id] = {"path": path, "callback": callback}


async def webhook_unregister(trigger_config):
    logger = Logger()
    logger.info("Removing webhook endpoint", {"id": trigger_config["id"]})
    webhook_endpoints.pop(trigger_config["id"], None)


class WebhookHandler(TriggerHandler):
    async def register_trigger(self, config):
        await webhook_register(config.model_dump())

    async def unregister_trigger(self, config):
        await webhook_unregister(config.model_dump())


iii.register_trigger_type({
    "id": "webhook",
    "description": "Fires when an external service sends an HTTP POST to the registered endpoint",
}, WebhookHandler())

# ---
# Custom trigger type — External subscription
# Bridges an event source that already pushes topic messages.
# ---
class PushBus:
    def __init__(self):
        self._handlers = defaultdict(list)

    def subscribe(self, topic, handler):
        self._handlers[topic].append(handler)

    def unsubscribe(self, topic, handler):
        self._handlers[topic] = [h for h in self._handlers[topic] if h is not handler]

    async def publish(self, topic, message):
        for handler in list(self._handlers[topic]):
            await handler(message)


push_bus = PushBus()
topic_subscriptions = {}


async def topic_register(trigger_config):
    trigger_id = trigger_config["id"]
    function_id = trigger_config["function_id"]
    config = trigger_config["config"]
    topic = config["topic"]

    async def handler(message):
        await iii.trigger_async({
            "function_id": function_id,
            "payload": {"source": "topic-subscription", "trigger_id": trigger_id, "topic": topic, "data": message},
        })

    push_bus.subscribe(topic, handler)
    topic_subscriptions[trigger_id] = (topic, handler)


async def topic_unregister(trigger_config):
    subscription = topic_subscriptions.pop(trigger_config["id"], None)
    if subscription:
        topic, handler = subscription
        push_bus.unsubscribe(topic, handler)


class TopicSubscriptionHandler(TriggerHandler):
    async def register_trigger(self, config):
        await topic_register(config.model_dump())

    async def unregister_trigger(self, config):
        await topic_unregister(config.model_dump())


iii.register_trigger_type({
    "id": "topic-subscription",
    "description": "Fires when an external event bus publishes to a topic",
}, TopicSubscriptionHandler())

# ---
# Handler function — processes events from any custom trigger above
# ---


async def on_event(data):
    logger = Logger()
    logger.info("Custom trigger fired", {"source": data["source"], "trigger_id": data["trigger_id"]})
    return {"received": True, "source": data["source"]}


iii.register_function("custom-triggers::on-event", on_event)

# ---
# Bind triggers using the custom types defined above
# ---
iii.register_trigger({
    "type": "webhook",
    "function_id": "custom-triggers::on-event",
    "config": {"path": "/hooks/github"},
})

iii.register_trigger({
    "type": "topic-subscription",
    "function_id": "custom-triggers::on-event",
    "config": {"topic": "orders.created"},
})


async def main():
    while True:
        await asyncio.sleep(60)


if __name__ == "__main__":
    asyncio.run(main())
