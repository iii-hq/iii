"""Delete Todo Step."""

from typing import Any

from motia import ApiRequest, ApiResponse, Stream, http, logger

todo_stream: Stream[dict[str, Any]] = Stream("todo")

config = {
    "name": "DeleteTodo",
    "description": "Delete a todo item",
    "flows": ["todo-app"],
    "triggers": [
        http("DELETE", "/todo"),
    ],
    "enqueues": [],
}


def handler(request: ApiRequest[dict[str, Any]]) -> ApiResponse[Any]:
    """Handle delete todo request."""
    body = request.body or {}
    todo_id = body.get("todo_id")

    logger.info("Deleting todo", body)

    if not todo_id:
        logger.error("todo_id is required")
        return ApiResponse(status=400, body={"error": "todo_id is required"})

    todo_stream.delete("inbox", todo_id)

    logger.info("Todo deleted successfully", {"todo_id": todo_id})

    return ApiResponse(status=200, body={"success": True})
