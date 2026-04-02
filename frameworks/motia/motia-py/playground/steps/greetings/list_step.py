"""List greetings API step."""

from typing import Any

from motia import ApiRequest, ApiResponse, Stream, http, logger

GREETINGS_GROUP_ID = "default"
greetings_stream: Stream[dict[str, Any]] = Stream("greetings")


config = {
    "name": "ListGreetings",
    "description": "List greetings stored in the stream",
    "triggers": [
        http("GET", "/greetings"),
    ],
    "enqueues": [],
}


def handler(request: ApiRequest[Any]) -> ApiResponse[dict[str, Any]]:
    """Handle list greetings requests."""
    _ = request
    greetings = greetings_stream.get_group(GREETINGS_GROUP_ID)
    logger.info("Greetings listed", {"count": len(greetings)})
    return ApiResponse(status=200, body={"greetings": greetings})
