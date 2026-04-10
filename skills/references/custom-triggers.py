"""
Pattern: Custom Triggers
Comparable to: Custom event adapters, webhook connectors, polling integrators

Demonstrates how to define entirely new trigger types beyond the built-in
http, durable:subscriber, cron, state, and subscribe triggers. A custom trigger type
registers handler callbacks that the engine invokes when triggers of that
type are created or removed, letting you bridge any external event source
(webhooks, pollers) into the iii function graph.

Note: File watcher is omitted — it requires the watchdog dependency.
Polling uses asyncio.create_task instead of setInterval.

How-to references:
  - Custom trigger types: https://iii.dev/docs/how-to/create-custom-trigger-type
"""

import asyncio
import os

from iii import InitOptions, Logger, TriggerAction, register_worker

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


iii.register_trigger_type({
    "id": "webhook",
    "description": "Fires when an external service sends an HTTP POST to the registered endpoint",
    "handler": {
        "register_trigger": webhook_register,
        "unregister_trigger": webhook_unregister,
    },
})

# ---
# Custom trigger type — Polling with ETag
# Periodically fetches a URL and fires only when the content changes.
# Uses asyncio.create_task for the polling loop instead of setInterval.
# ---
pollers = {}


async def _poll_loop(trigger_id, function_id, url, interval_ms):
    import urllib.request
    import json

    last_etag = None
    interval_s = interval_ms / 1000

    while True:
        try:
            req = urllib.request.Request(url, method="GET")
            if last_etag:
                req.add_header("If-None-Match", last_etag)

            resp = await asyncio.to_thread(urllib.request.urlopen, req)

            if resp.status == 304:
                await asyncio.sleep(interval_s)
                continue

            etag = resp.headers.get("ETag")
            if etag and etag != last_etag:
                last_etag = etag
                body = json.loads(resp.read().decode())

                await iii.trigger_async({
                    "function_id": function_id,
                    "payload": {"source": "polling", "trigger_id": trigger_id, "etag": etag, "data": body},
                })
        except asyncio.CancelledError:
            break
        except Exception as err:
            logger = Logger()
            logger.error("Polling failed", {"id": trigger_id, "url": url, "error": str(err)})

        await asyncio.sleep(interval_s)


async def polling_register(trigger_config):
    trigger_id = trigger_config["id"]
    function_id = trigger_config["function_id"]
    config = trigger_config["config"]
    url = config["url"]
    interval_ms = config.get("interval_ms", 30000)

    task = asyncio.create_task(_poll_loop(trigger_id, function_id, url, interval_ms))
    pollers[trigger_id] = task


async def polling_unregister(trigger_config):
    task = pollers.pop(trigger_config["id"], None)
    if task:
        task.cancel()


iii.register_trigger_type({
    "id": "polling",
    "description": "Polls a URL at a fixed interval and fires when the ETag changes",
    "handler": {
        "register_trigger": polling_register,
        "unregister_trigger": polling_unregister,
    },
})

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
    "type": "polling",
    "function_id": "custom-triggers::on-event",
    "config": {"url": "https://api.example.com/status", "interval_ms": 60000},
})


async def main():
    while True:
        await asyncio.sleep(60)


if __name__ == "__main__":
    asyncio.run(main())
