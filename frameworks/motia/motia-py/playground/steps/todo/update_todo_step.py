"""Update Todo Step."""

from typing import Any

from motia import ApiRequest, ApiResponse, Stream, http, logger

todo_stream: Stream[dict[str, Any]] = Stream("todo")

config = {
    "name": "UpdateTodo",
    "description": "Update an existing todo item",
    "flows": ["todo-app"],
    "triggers": [
        http("PUT", "/todo"),
    ],
    "enqueues": [],
}


def handler(request: ApiRequest[dict[str, Any]]) -> ApiResponse[Any]:
    """Handle update todo request."""
    body = request.body or {}
    todo_id = body.get("todo_id")

    logger.info("Updating todo", body)

    if not todo_id:
        logger.error("todo_id is required")
        return ApiResponse(status=400, body={"error": "todo_id is required"})

    existing_todo = todo_stream.get("inbox", todo_id)

    if not existing_todo:
        logger.error("Todo not found")
        return ApiResponse(status=404, body={"error": "Todo not found"})

    updated_todo = {**existing_todo, **body}
    todo = todo_stream.set("inbox", todo_id, updated_todo)

    logger.info("Todo updated successfully", {"todo_id": todo_id})

    return ApiResponse(status=200, body=todo)
