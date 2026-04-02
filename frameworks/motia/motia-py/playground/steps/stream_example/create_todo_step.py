"""Step that creates todos in a stream."""

import uuid
from typing import Any

from motia import Stream, http, logger

todo_stream: Stream[dict[str, Any]] = Stream("todo")

config = {
    "name": "create-todo",
    "description": "API endpoint to create a todo item",
    "triggers": [
        http(
            "POST",
            "/todos",
            body_schema={
                "type": "object",
                "properties": {
                    "description": {"type": "string"},
                    "group_id": {"type": "string"},
                },
            },
        ),
    ],
    "flows": ["stream-example"],
}


def handler(request: Any) -> dict[str, Any]:
    """Create a new todo item."""
    todo_id = str(uuid.uuid4())
    body = request.body or {}
    description = body.get("description", "New todo")
    group_id = body.get("group_id", "inbox")

    todo = {
        "id": todo_id,
        "description": description,
        "completed": False,
    }

    todo_stream.set(group_id, todo_id, todo)

    logger.info(f"Created todo {todo_id} in group {group_id}")

    return {
        "status": 201,
        "body": todo,
    }
