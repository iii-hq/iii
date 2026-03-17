"""Process Order Step - receives order.created events.

Demonstrates trace context propagation across steps.
"""

from typing import Any

from motia import FlowContext, Stream, enqueue, logger, queue, stateManager

order_stream: Stream[dict[str, Any]] = Stream("orders")

config = {
    "name": "ProcessOrder",
    "description": "Process a created order - trace continues from CreateOrder",
    "flows": ["otel-example"],
    "triggers": [
        queue("order.created"),
    ],
    "enqueues": ["order.processed"],
}


def handler(data: Any, ctx: FlowContext[Any]) -> None:
    order = data.get("data", {}) if isinstance(data, dict) else {}
    order_id = order.get("id", "unknown")

    logger.info("Processing order", {"order_id": order_id, "trace_id": ctx.trace_id})

    current = order_stream.get("pending", order_id)

    if current:
        stateManager.set("order_processing", order_id, {"status": "processing"})
        processed_order = {**current, "status": "processed"}
        order_stream.set("processed", order_id, processed_order)
        enqueue({"topic": "order.processed", "data": processed_order})
        logger.info("Order processed", {"order_id": order_id})
    else:
        logger.warn("Order not found in stream", {"order_id": order_id})
