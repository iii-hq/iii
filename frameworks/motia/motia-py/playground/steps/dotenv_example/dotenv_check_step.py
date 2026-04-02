"""Returns environment variables loaded from .env file."""

import os
from typing import Any

from motia import ApiRequest, ApiResponse, http, logger

config = {
    "name": "DotenvCheck",
    "description": "Returns environment variables loaded from .env file",
    "triggers": [
        http("GET", "/dotenv-check"),
    ],
    "enqueues": [],
    "flows": ["dotenv-example"],
}


def handler(request: ApiRequest[Any]) -> ApiResponse[dict[str, Any]]:
    """Return env vars loaded from .env."""
    greeting_prefix = os.environ.get("GREETING_PREFIX", "NOT_SET")
    app_name = os.environ.get("APP_NAME", "NOT_SET")
    has_secret_key = bool(os.environ.get("SECRET_KEY"))

    logger.info("Dotenv check", {"greeting_prefix": greeting_prefix, "app_name": app_name, "has_secret_key": has_secret_key})

    return ApiResponse(
        status=200,
        body={
            "greeting_prefix": greeting_prefix,
            "app_name": app_name,
            "has_secret_key": has_secret_key,
        },
    )
