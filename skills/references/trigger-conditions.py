"""
Pattern: Trigger Conditions
Comparable to: Event filters, guard clauses, conditional routing

A trigger condition is a regular function that returns a boolean. When
attached to a trigger via condition_function_id, the engine calls the
condition first — if it returns true the handler runs, otherwise the
event is silently skipped. The condition receives the same event data
as the handler.

How-to references:
  - Trigger conditions: https://iii.dev/docs/how-to/use-trigger-conditions
"""

import asyncio
import os
from datetime import datetime, timezone

from iii import InitOptions, Logger, TriggerAction, register_worker

iii = register_worker(
    address=os.environ.get("III_ENGINE_URL", "ws://localhost:49134"),
    options=InitOptions(worker_name="trigger-conditions"),
)

# ---
# Example 1 — State trigger with a high-value order condition
# Only fires the handler when the order total exceeds $500.
# ---


async def is_high_value(data):
    value = data.get("value") or {}
    return value.get("total", 0) > 500


iii.register_function("conditions::is-high-value", is_high_value)


async def flag_high_value(data):
    logger = Logger()
    logger.info("High-value order detected", {"key": data["key"], "total": data["value"]["total"]})

    await iii.trigger_async({
        "function_id": "state::update",
        "payload": {
            "scope": "orders",
            "key": data["key"],
            "ops": [{"type": "set", "path": "flagged", "value": True}],
        },
    })

    return {"flagged": True, "order_id": data["key"]}


iii.register_function("orders::flag-high-value", flag_high_value)

iii.register_trigger({
    "type": "state",
    "function_id": "orders::flag-high-value",
    "config": {
        "scope": "orders",
        "condition_function_id": "conditions::is-high-value",
    },
})

# ---
# Example 2 — HTTP trigger with request validation condition
# Rejects requests missing a required API key header.
# ---


async def has_api_key(data):
    headers = data.get("headers") or {}
    api_key = headers.get("x-api-key")
    return isinstance(api_key, str) and len(api_key) > 0


iii.register_function("conditions::has-api-key", has_api_key)


async def protected_endpoint(data):
    logger = Logger()
    logger.info("Authenticated request", {"path": data.get("path")})
    return {"message": "Access granted", "user": data["headers"]["x-api-key"]}


iii.register_function("api::protected-endpoint", protected_endpoint)

iii.register_trigger({
    "type": "http",
    "function_id": "api::protected-endpoint",
    "config": {
        "api_path": "/api/protected",
        "http_method": "GET",
        "condition_function_id": "conditions::has-api-key",
    },
})

# ---
# Example 3 — Queue trigger with event type filter condition
# Only processes messages whose `event_type` is "order.placed".
# ---


async def is_order_placed(data):
    return data.get("event_type") == "order.placed"


iii.register_function("conditions::is-order-placed", is_order_placed)


async def on_placed(data):
    logger = Logger()
    logger.info("Processing order.placed event", {"orderId": data["order_id"]})

    await iii.trigger_async({
        "function_id": "orders::fulfill",
        "payload": {"order_id": data["order_id"]},
        "action": TriggerAction.Enqueue({"queue": "fulfillment"}),
    })

    return {"processed": True, "order_id": data["order_id"]}


iii.register_function("orders::on-placed", on_placed)


async def fulfill(data):
    logger = Logger()
    logger.info("Fulfilling order", {"orderId": data["order_id"]})
    return {"fulfilled": True}


iii.register_function("orders::fulfill", fulfill)

iii.register_trigger({
    "type": "durable:subscriber",
    "function_id": "orders::on-placed",
    "config": {
        "topic": "order-events",
        "condition_function_id": "conditions::is-order-placed",
    },
})

# ---
# Example 4 — Condition with shared data
# The condition and handler receive identical event data, so a condition can
# enrich or validate any field the handler will use.
# ---


async def is_weekday(data):
    day = datetime.now(timezone.utc).weekday()
    return day < 5


iii.register_function("conditions::is-weekday", is_weekday)


async def weekday_digest(data):
    logger = Logger()
    logger.info("Running weekday digest")
    return {"generated": True}


iii.register_function("reports::weekday-digest", weekday_digest)

iii.register_trigger({
    "type": "cron",
    "function_id": "reports::weekday-digest",
    "config": {
        "expression": "0 0 8 * * * *",
        "condition_function_id": "conditions::is-weekday",
    },
})


async def main():
    while True:
        await asyncio.sleep(60)


if __name__ == "__main__":
    asyncio.run(main())
