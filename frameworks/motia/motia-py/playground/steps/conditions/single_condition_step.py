"""Test single condition on event trigger."""

from typing import Any

from motia import FlowContext, logger, queue


def is_high_value(input: Any, ctx: FlowContext[Any]) -> bool:
    """Check if order value is high."""
    _ = ctx
    data = input or {}
    amount = data.get("amount", 0) if isinstance(data, dict) else 0
    return amount > 1000


config = {
    "name": "SingleConditionTest",
    "description": "Test single condition on event trigger",
    "triggers": [
        queue("order.created", condition=is_high_value),
    ],
    "enqueues": ["order.processed"],
}


def handler(input: Any) -> None:
    """Handle orders that pass single condition."""
    logger.info("Processing high-value order", {"data": input})
