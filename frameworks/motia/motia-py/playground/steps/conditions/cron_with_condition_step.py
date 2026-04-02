"""Test condition on cron trigger."""

from typing import Any

from motia import FlowContext, cron, logger


def is_business_hours(input: Any, ctx: FlowContext[Any]) -> bool:
    """Check if current time is business hours."""
    _ = input
    _ = ctx
    from datetime import datetime

    now = datetime.now()
    return 9 <= now.hour < 17


config = {
    "name": "CronWithCondition",
    "description": "Test condition on cron trigger",
    "triggers": [
        cron("5 * * * * * *", condition=is_business_hours),
    ],
    "enqueues": [],
}


def handler(input: None) -> None:
    """Handle cron that fires only during business hours."""
    _ = input
    logger.info("Running business hours task")
