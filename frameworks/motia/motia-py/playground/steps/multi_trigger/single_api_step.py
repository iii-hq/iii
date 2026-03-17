"""Test single API trigger."""

from typing import Any

from motia import ApiRequest, ApiResponse, http, logger

config = {
    "name": "SingleApiTrigger",
    "description": "Test single API trigger",
    "triggers": [
        http("GET", "/test/single"),
    ],
    "enqueues": [],
}


def handler(request: ApiRequest[Any]) -> ApiResponse[Any]:
    """Handle single API trigger."""
    logger.info("Single API trigger fired")
    return ApiResponse(status=200, body={"message": "Single API trigger works"})
