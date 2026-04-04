"""Test multiple conditions (AND logic) on event trigger."""

from typing import Any

from motia import ApiRequest, FlowContext, logger, queue


def is_high_value(input: Any, ctx: FlowContext[Any]) -> bool:
    """Check if order value is high."""
    _ = ctx
    data = input or {}
    amount = data.get("amount", 0) if isinstance(data, dict) else 0
    return amount > 1000


def is_verified_user(input: Any, ctx: FlowContext[Any]) -> bool:
    """Check if user is verified."""
    if isinstance(input, ApiRequest):
        body = input.body or {}
        user = body.get("user", {}) if isinstance(body, dict) else {}
        return user.get("verified", False)
    data = input or {}
    user = data.get("user", {}) if isinstance(data, dict) else {}
    return user.get("verified", False)


def is_domestic(input: Any, ctx: FlowContext[Any]) -> bool:
    """Check if order is domestic."""
    _ = ctx
    data = input or {}
    country = data.get("country") if isinstance(data, dict) else None
    return country in ["US", "CA"]


def all_premium_checks(input: Any, ctx: FlowContext[Any]) -> bool:
    """Combined check for premium orders."""
    return is_high_value(input, ctx) and is_verified_user(input, ctx) and is_domestic(input, ctx)


config = {
    "name": "MultipleConditionsTest",
    "description": "Test multiple conditions (AND logic)",
    "triggers": [
        queue("order.created", condition=all_premium_checks),
    ],
    "enqueues": ["premium.order.processed"],
}


def handler(input: Any) -> None:
    """Handle orders that pass all conditions."""
    logger.info("Processing premium order", {"data": input})
