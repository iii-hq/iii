"""Test conditions on API trigger."""

from typing import Any

from motia import ApiRequest, ApiResponse, FlowContext, http, logger


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


def api_premium_check(input: Any, ctx: FlowContext[Any]) -> bool:
    """Combined check for API premium orders."""
    return is_high_value(input, ctx) and is_verified_user(input, ctx)


config = {
    "name": "ApiWithConditions",
    "description": "Test conditions on API trigger",
    "triggers": [
        http("POST", "/orders/premium", condition=api_premium_check),
    ],
    "enqueues": [],
}


def handler(request: ApiRequest[Any]) -> ApiResponse[Any]:
    """Handle API requests that pass conditions."""
    logger.info("Processing premium order via API")
    return ApiResponse(status=200, body={"message": "Premium order processed", "data": request.body})
