"""Test single event trigger."""

from typing import Any

from motia import logger, queue

config = {
    "name": "SingleEventTrigger",
    "description": "Test single event trigger",
    "triggers": [
        queue("test.event"),
    ],
    "enqueues": ["test.processed"],
}


def handler(input: Any) -> None:
    """Handle single event trigger."""
    logger.info("Single event trigger fired", {"data": input})
