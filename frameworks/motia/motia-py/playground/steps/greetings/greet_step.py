"""Greeting API step."""

from typing import Any

from motia import ApiRequest, ApiResponse, Stream, http, logger

GREETINGS_GROUP_ID = "default"
greetings_stream: Stream[dict[str, Any]] = Stream("greetings")


def _first_param(value: str | list[str] | None) -> str | None:
    if value is None:
        return None
    if isinstance(value, list):
        return value[0] if value else None
    return value


config = {
    "name": "Greet",
    "description": "Greet and store the greeting in a stream",
    "triggers": [
        http("GET", "/greet"),
    ],
    "enqueues": [],
}


def handler(request: ApiRequest[Any]) -> ApiResponse[dict[str, Any]]:
    """Handle greeting requests."""
    name = _first_param(request.query_params.get("name")) or "world"
    greeting = {"name": name, "message": f"Hello, {name}!"}

    greetings_stream.set(GREETINGS_GROUP_ID, name, greeting)
    logger.info("Greeting stored", {"name": name})

    return ApiResponse(status=200, body=greeting)
