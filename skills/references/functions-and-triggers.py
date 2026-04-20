"""
Pattern: Functions & Triggers (Python)
Comparable to: Core primitives of iii

Demonstrates every fundamental building block in Python: registering functions,
binding triggers of each built-in type (http, durable:subscriber, cron, state, subscribe),
cross-function invocation, fire-and-forget calls, and external HTTP-invoked
functions via HttpInvocationConfig.

How-to references:
  - Functions & Triggers: https://iii.dev/docs/how-to/use-functions-and-triggers
"""

import asyncio
import os

from iii import InitOptions, Logger, TriggerAction, register_worker

engine_url = os.environ.get("III_ENGINE_URL", "ws://localhost:49134")
iii = register_worker(
    address=engine_url,
    options=InitOptions(worker_name="functions-and-triggers"),
)

# ---------------------------------------------------------------------------
# 1. Register a simple function
# ---------------------------------------------------------------------------
async def validate_order(data):
    logger = Logger(service_name="orders::validate")
    logger.info("Validating order", {"order_id": data.get("order_id")})

    if not data.get("order_id") or not data.get("items"):
        return {"valid": False, "reason": "Missing order_id or items"}
    return {"valid": True, "order_id": data["order_id"]}

iii.register_function("orders::validate", validate_order)

# ---------------------------------------------------------------------------
# 2. HTTP trigger — expose a function as a REST endpoint
# ---------------------------------------------------------------------------
iii.register_trigger({
    "type": "http",
    "function_id": "orders::validate",
    "config": {"api_path": "/orders/validate", "http_method": "POST"},
})

# ---------------------------------------------------------------------------
# 3. Queue trigger — process items from a named queue
# ---------------------------------------------------------------------------
async def fulfill_order(data):
    logger = Logger(service_name="orders::fulfill")
    logger.info("Fulfilling order", {"order_id": data.get("order_id")})
    return {"fulfilled": True, "order_id": data["order_id"]}

iii.register_function("orders::fulfill", fulfill_order)

iii.register_trigger({
    "type": "durable:subscriber",
    "function_id": "orders::fulfill",
    "config": {"topic": "fulfillment"},
})

# ---------------------------------------------------------------------------
# 4. Cron trigger — run a function on a schedule
# ---------------------------------------------------------------------------
async def daily_summary(_data):
    logger = Logger(service_name="reports::daily-summary")
    logger.info("Generating daily summary")
    return {"generated_at": "now"}

iii.register_function("reports::daily-summary", daily_summary)

iii.register_trigger({
    "type": "cron",
    "function_id": "reports::daily-summary",
    "config": {"expression": "0 0 9 * * * *"},
})

# ---------------------------------------------------------------------------
# 5. State trigger — react when a state scope/key changes
# ---------------------------------------------------------------------------
async def on_status_change(data):
    logger = Logger(service_name="orders::on-status-change")
    logger.info("Order status changed", {"key": data.get("key"), "value": data.get("value")})
    return {"notified": True}

iii.register_function("orders::on-status-change", on_status_change)

iii.register_trigger({
    "type": "state",
    "function_id": "orders::on-status-change",
    "config": {"scope": "orders"},
})

# ---------------------------------------------------------------------------
# 6. Subscribe trigger — listen for pubsub messages on a topic
# ---------------------------------------------------------------------------
async def on_order_complete(data):
    logger = Logger(service_name="notifications::on-order-complete")
    logger.info("Order completed event received", {"order_id": data.get("order_id")})
    return {"processed": True}

iii.register_function("notifications::on-order-complete", on_order_complete)

iii.register_trigger({
    "type": "subscribe",
    "function_id": "notifications::on-order-complete",
    "config": {"topic": "orders.completed"},
})

# ---------------------------------------------------------------------------
# 7. Cross-function invocation — one function calling another
# ---------------------------------------------------------------------------
async def create_order(data):
    logger = Logger(service_name="orders::create")

    # Synchronous call — blocks until validate returns
    validation = await iii.trigger_async({
        "function_id": "orders::validate",
        "payload": {"order_id": data.get("order_id"), "items": data.get("items")},
    })

    if not validation.get("valid"):
        return {"error": validation.get("reason")}

    # Fire-and-forget — send a notification without waiting
    await iii.trigger_async({
        "function_id": "notifications::on-order-complete",
        "payload": {"order_id": data.get("order_id")},
        "action": TriggerAction.Void(),
    })

    # Enqueue — durable async handoff to fulfillment
    await iii.trigger_async({
        "function_id": "orders::fulfill",
        "payload": {"order_id": data.get("order_id"), "items": data.get("items")},
        "action": TriggerAction.Enqueue({"queue": "fulfillment"}),
    })

    return {"order_id": data.get("order_id"), "status": "accepted"}

iii.register_function("orders::create", create_order)

iii.register_trigger({
    "type": "http",
    "function_id": "orders::create",
    "config": {"api_path": "/orders", "http_method": "POST"},
})

# ---------------------------------------------------------------------------
# 8. External HTTP-invoked function (HttpInvocationConfig)
# ---------------------------------------------------------------------------
iii.register_function({
    "id": "external::payment-gateway",
    "invocation": {
        "url": "https://api.stripe.com/v1/charges",
        "method": "POST",
        "timeout_ms": 10000,
        "auth": {
            "type": "bearer",
            "token": os.environ.get("STRIPE_API_KEY", ""),
        },
    },
})

# Keep the process alive for event processing
async def main():
    while True:
        await asyncio.sleep(60)

if __name__ == "__main__":
    asyncio.run(main())
