"""Test multiple API triggers."""

from typing import Any

from motia import ApiRequest, ApiResponse, FlowContext, http, logger

config = {
    "name": "MultipleApis",
    "description": "Test multiple API triggers",
    "triggers": [
        http("GET", "/test/api/1"),
        http("POST", "/test/api/2"),
        http("PUT", "/test/api/3"),
    ],
    "enqueues": [],
}


def handler(request: ApiRequest[Any], ctx: FlowContext[Any]) -> ApiResponse[Any]:
    """Handle multiple API triggers."""
    logger.info("Multiple APIs trigger fired", {"path": ctx.trigger.path, "method": ctx.trigger.method})
    return ApiResponse(status=200, body={"message": "Multiple APIs trigger works"})
