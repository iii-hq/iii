"""
Pattern: State Reactions
Comparable to: Firebase onSnapshot, Convex mutations

Register functions that fire automatically when state changes
in a given scope. Optionally filter with a condition function
that returns a boolean.

How-to references:
  - State reactions: https://iii.dev/docs/how-to/react-to-state-changes
"""

import asyncio
import os
import time
from datetime import datetime, timezone

from iii import InitOptions, Logger, TriggerAction, register_worker

iii = register_worker(
    address=os.environ.get("III_ENGINE_URL", "ws://localhost:49134"),
    options=InitOptions(worker_name="state-reactions"),
)

# ---
# Basic state reaction — fires on ANY change in the 'orders' scope
# The handler receives: { new_value, old_value, key, event_type }
#   event_type: 'set' | 'update' | 'delete'
# ---


async def order_audit_log(event):
    logger = Logger()
    new_value = event.get("new_value")
    old_value = event.get("old_value")
    key = event.get("key")
    event_type = event.get("event_type")

    if not old_value:
        action = "created"
    elif not new_value:
        action = "deleted"
    else:
        action = "updated"

    logger.info("Order changed", {"key": key, "action": action, "event_type": event_type})

    audit_id = f"audit-{int(time.time() * 1000)}"
    await iii.trigger_async({
        "function_id": "state::set",
        "payload": {
            "scope": "order-audit",
            "key": audit_id,
            "value": {
                "auditId": audit_id,
                "orderKey": key,
                "action": action,
                "event_type": event_type,
                "before": old_value,
                "after": new_value,
                "timestamp": datetime.now(timezone.utc).isoformat(),
            },
        },
    })

    return {"auditId": audit_id, "action": action}


iii.register_function("reactions::order-audit-log", order_audit_log)

iii.register_trigger({
    "type": "state",
    "function_id": "reactions::order-audit-log",
    "config": {"scope": "orders"},
})

# ---
# Conditional reaction — only fires when condition function returns true
# The condition function receives the same event and must return a boolean.
# ---


async def high_value_alert_condition(event):
    new_value = event.get("new_value")
    return bool(new_value and new_value.get("total", 0) > 1000)


iii.register_function("reactions::high-value-alert-condition", high_value_alert_condition)


async def high_value_alert(event):
    logger = Logger()
    new_value = event.get("new_value")
    key = event.get("key")

    logger.info("High-value order detected", {"key": key, "total": new_value["total"]})

    iii.trigger({
        "function_id": "alerts::notify-manager",
        "payload": {
            "type": "high-value-order",
            "orderId": key,
            "total": new_value["total"],
            "customer": new_value.get("customer"),
        },
        "action": TriggerAction.Enqueue({"queue": "alerts"}),
    })

    return {"alerted": True, "orderId": key}


iii.register_function("reactions::high-value-alert", high_value_alert)

iii.register_trigger({
    "type": "state",
    "function_id": "reactions::high-value-alert",
    "config": {
        "scope": "orders",
        "condition_function_id": "reactions::high-value-alert-condition",
    },
})

# ---
# Multiple independent reactions to the same scope
# Each trigger registers a separate function on the same scope.
# All registered reactions fire independently on every matching change.
# ---


async def order_metrics(event):
    new_value = event.get("new_value")
    old_value = event.get("old_value")

    ops = []

    if new_value and not old_value:
        ops.append({"type": "increment", "path": "total_orders", "by": 1})
        ops.append({"type": "increment", "path": "total_revenue", "by": new_value.get("total", 0)})

    if not new_value and old_value:
        ops.append({"type": "increment", "path": "total_orders", "by": -1})
        ops.append({"type": "increment", "path": "total_revenue", "by": -(old_value.get("total", 0))})

    if ops:
        await iii.trigger_async({
            "function_id": "state::update",
            "payload": {"scope": "order-metrics", "key": "global", "ops": ops},
        })


iii.register_function("reactions::order-metrics", order_metrics)

iii.register_trigger({
    "type": "state",
    "function_id": "reactions::order-metrics",
    "config": {"scope": "orders"},
})


async def order_live_feed(event):
    new_value = event.get("new_value")
    old_value = event.get("old_value")
    key = event.get("key")

    if not old_value:
        action = "created"
    elif not new_value:
        action = "deleted"
    else:
        action = "updated"

    iii.trigger({
        "function_id": "stream::send",
        "payload": {
            "stream_name": "orders-live",
            "group_id": "dashboard",
            "id": f"evt-{int(time.time() * 1000)}",
            "event_type": "order_changed",
            "data": {"action": action, "key": key, "order": new_value},
        },
        "action": TriggerAction.Void(),
    })


iii.register_function("reactions::order-live-feed", order_live_feed)

iii.register_trigger({
    "type": "state",
    "function_id": "reactions::order-live-feed",
    "config": {"scope": "orders"},
})


async def main():
    while True:
        await asyncio.sleep(60)


if __name__ == "__main__":
    asyncio.run(main())
