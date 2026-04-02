"""Test multiple triggers with different conditions."""

from typing import Any

from motia import ApiRequest, ApiResponse, FlowContext, http, logger, queue


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


config = {
    "name": "MixedTriggersWithConditions",
    "description": "Test multiple triggers each with different conditions",
    "triggers": [
        queue("order.created", condition=is_high_value),
        http("POST", "/orders/manual", condition=is_verified_user),
    ],
    "enqueues": ["order.processed"],
}


def handler(input_data: Any, ctx: FlowContext[Any]) -> Any:
    """Dispatch to handler based on trigger type."""

    def _event_handler(input: Any) -> None:
        logger.info("Processing order (event)", {"data": input, "topic": ctx.trigger.topic})

    def _api_handler(request: ApiRequest[Any]) -> ApiResponse[Any]:
        logger.info("Processing order (api)", {"path": ctx.trigger.path, "method": ctx.trigger.method})
        return ApiResponse(status=200, body={"message": "Order processed via API"})

    return ctx.match(
        {
            "queue": _event_handler,
            "http": _api_handler,
        },
    )
