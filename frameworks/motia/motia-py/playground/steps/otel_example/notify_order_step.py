"""Notify Order Step - receives order.processed events.

Final step in the flow. The full trace tree is visible via engine.traces.list.
"""

from typing import Any

from motia import FlowContext, logger, queue, stateManager

config = {
    "name": "NotifyOrder",
    "description": "Send notification for processed order",
    "flows": ["otel-example"],
    "triggers": [
        queue("order.processed"),
    ],
    "enqueues": [],
}


def handler(data: Any, ctx: FlowContext[Any]) -> None:
    order = data.get("data", {}) if isinstance(data, dict) else {}
    order_id = order.get("id", "unknown")

    logger.info(
        "Sending notification for order",
        {
            "order_id": order_id,
            "trace_id": ctx.trace_id,
        },
    )

    stateManager.set(
        "notifications",
        order_id,
        {
            "order_id": order_id,
            "notified": True,
            "trace_id": ctx.trace_id,
        },
    )

    logger.info("Notification sent", {"order_id": order_id})
