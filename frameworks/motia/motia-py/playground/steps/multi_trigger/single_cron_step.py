"""Test single cron trigger."""

from typing import Any

from motia import cron, logger

config = {
    "name": "SingleCronTrigger",
    "description": "Test single cron trigger",
    "triggers": [
        cron("5 * * * * *"),
    ],
    "enqueues": [],
}


def handler(input: None) -> None:
    """Handle single cron trigger."""
    _ = input
    logger.info("Single cron trigger fired")
