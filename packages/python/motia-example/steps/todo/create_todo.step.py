"""Create Todo Step."""

import random
import string
from datetime import datetime
from typing import Any

from motia import ApiRequest, ApiResponse, ApiTrigger, FlowContext, StepConfig, Stream, step_wrapper

todo_stream: Stream[dict[str, Any]] = Stream("todo")


config = StepConfig(
    name="CreateTodo",
    description="Create a new todo item",
    flows=["todo-app"],
    triggers=[
        ApiTrigger(path="/todo", method="POST"),
    ],
    emits=[],
)


async def handler(request: ApiRequest[dict[str, Any]], ctx: FlowContext[Any]) -> ApiResponse[Any]:
    """Handle create todo request."""
    ctx.logger.info("Creating new todo", request.body)

    body = request.body or {}
    description = body.get("description")
    due_date = body.get("due_date")

    if not description:
        return ApiResponse(status=400, body={"error": "Description is required"})

    # Generate unique ID
    random_suffix = "".join(random.choices(string.ascii_lowercase + string.digits, k=7))
    todo_id = f"todo-{int(datetime.now().timestamp() * 1000)}-{random_suffix}"

    new_todo: dict[str, Any] = {
        "id": todo_id,
        "description": description,
        "created_at": datetime.now().isoformat(),
        "due_date": due_date,
        "completed_at": None,
    }

    todo = await todo_stream.set("inbox", todo_id, new_todo)

    ctx.logger.info("Todo created successfully", {"todo_id": todo_id})

    return ApiResponse(status=200, body=todo)


step_wrapper(config, __file__, handler)
