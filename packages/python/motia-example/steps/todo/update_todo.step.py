"""Update Todo Step."""

from typing import Any

from motia import ApiResponse, ApiTrigger, FlowContext, StepConfig, Stream, TriggerInput, step_wrapper

todo_stream: Stream[dict[str, Any]] = Stream("todo")


config = StepConfig(
    name="UpdateTodo",
    description="Update an existing todo item",
    flows=["todo-app"],
    triggers=[
        ApiTrigger(path="/todo", method="PUT"),
    ],
    emits=[],
)


async def handler(input: TriggerInput[dict[str, Any]], ctx: FlowContext[Any]) -> ApiResponse[Any]:
    """Handle update todo request."""
    body = input.data or {}
    todo_id = body.get("todo_id")

    ctx.logger.info("Updating todo", body)

    if not todo_id:
        ctx.logger.error("todo_id is required")
        return ApiResponse(status=400, body={"error": "todo_id is required"})

    existing_todo = await todo_stream.get("inbox", todo_id)

    if not existing_todo:
        ctx.logger.error("Todo not found")
        return ApiResponse(status=404, body={"error": "Todo not found"})

    # Merge existing todo with updates
    updated_todo = {**existing_todo, **body}
    todo = await todo_stream.set("inbox", todo_id, updated_todo)

    ctx.logger.info("Todo updated successfully", {"todo_id": todo_id})

    return ApiResponse(status=200, body=todo)


step_wrapper(config, __file__, handler)
