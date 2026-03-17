"""Get greeting API step."""

from typing import Any

from motia import ApiRequest, ApiResponse, FlowContext, Stream, http

GREETINGS_GROUP_ID = "default"
greetings_stream: Stream[dict[str, Any]] = Stream("greetings")


config = {
    "name": "GetGreeting",
    "description": "Get a greeting from the stream",
    "triggers": [
        http("GET", "/greetings/:name"),
    ],
    "enqueues": [],
}


def handler(request: ApiRequest[Any], ctx: FlowContext[Any]) -> ApiResponse[dict[str, Any]]:
    """Handle get greeting requests."""
    name = request.path_params.get("name")
    if not name:
        return ApiResponse(status=400, body={"error": "missing name"})

    greeting = greetings_stream.get(GREETINGS_GROUP_ID, name)
    if greeting is None:
        return ApiResponse(status=404, body={"error": "not found"})

    _ = ctx
    return ApiResponse(status=200, body=greeting)
