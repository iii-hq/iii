"""Test multiple event triggers."""

from typing import Any

from motia import FlowContext, logger, queue

config = {
    "name": "MultipleEvents",
    "description": "Test multiple event triggers",
    "triggers": [
        queue("test.event.1"),
        queue("test.event.2"),
        queue("test.event.3"),
    ],
    "enqueues": ["test.events.processed"],
}


def handler(input: Any, ctx: FlowContext[Any]) -> None:
    """Handle multiple event triggers."""
    logger.info("Multiple events trigger fired", {"data": input, "topic": ctx.trigger.topic})
