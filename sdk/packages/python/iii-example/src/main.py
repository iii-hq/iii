import json
import os
import random
import string
import time
import urllib.request
from datetime import datetime, timezone
from typing import Any

from iii import ApiRequest, ApiResponse, InitOptions, register_worker

state: Any = None
streams: Any = None


def _generate_todo_id() -> str:
    suffix = "".join(random.choices(string.ascii_lowercase + string.digits, k=7))
    return f"todo-{int(time.time() * 1000)}-{suffix}"


def _setup(iii) -> None:
    from .hooks import use_api, use_functions_available
    from .state import State
    from .stream import StreamClient, register_streams
    from .trigger_types import setup as setup_trigger_types

    global state, streams
    state = State(iii)
    streams = StreamClient(iii)
    register_streams(iii)
    setup_trigger_types(iii)

    use_functions_available(
        iii,
        lambda functions: print(
            "--------------------------------\n"
            f"Functions available: {len(functions)}\n"
            "--------------------------------"
        )
    )

    use_api(
        iii,
        {"api_path": "todo", "http_method": "POST", "description": "Create a new todo", "metadata": {"tags": ["todo"]}},
        _create_todo,
    )

    use_api(
        iii,
        {"api_path": "todo", "http_method": "DELETE", "description": "Delete a todo", "metadata": {"tags": ["todo"]}},
        _delete_todo,
    )

    use_api(
        iii,
        {"api_path": "todo/:id", "http_method": "PUT", "description": "Update a todo", "metadata": {"tags": ["todo"]}},
        _update_todo,
    )

    use_api(
        iii,
        {"api_path": "state", "http_method": "POST", "description": "Set application state"},
        _create_state,
    )

    use_api(
        iii,
        {"api_path": "state/:id", "http_method": "GET", "description": "Get state by ID"},
        _get_state,
    )

    use_api(
        iii,
        {"api_path": "error-test", "http_method": "GET", "description": "Raises an error to test OTEL stack traces"},
        _error_test,
    )

    use_api(
        iii,
        {
            "api_path": "http-fetch",
            "http_method": "GET",
            "description": "Fetch a todo from JSONPlaceholder (tests urllib instrumentation)",
        },
        _fetch_example,
    )

    use_api(
        iii,
        {
            "api_path": "http-fetch",
            "http_method": "POST",
            "description": "Post data to httpbin (tests urllib instrumentation)",
        },
        _post_example,
    )


async def _create_todo(req: ApiRequest, logger) -> ApiResponse:
    logger.info("Creating new todo", {"body": req.body})

    description = req.body.get("description") if req.body else None
    due_date = req.body.get("dueDate") if req.body else None
    todo_id = _generate_todo_id()

    if not description:
        return ApiResponse(statusCode=400, body={"error": "Description is required"})

    new_todo = {
        "id": todo_id,
        "description": description,
        "createdAt": datetime.now(timezone.utc).isoformat(),
        "dueDate": due_date,
        "completedAt": None,
    }
    todo = await streams.set("todo", "inbox", todo_id, new_todo)
    return ApiResponse(statusCode=201, body=todo, headers={"Content-Type": "application/json"})


async def _delete_todo(req: ApiRequest, logger) -> ApiResponse:
    todo_id = req.body.get("todoId") if req.body else None

    logger.info("Deleting todo", {"body": req.body})

    if not todo_id:
        logger.error("todoId is required")
        return ApiResponse(statusCode=400, body={"error": "todoId is required"})

    await streams.delete("todo", "inbox", todo_id)

    logger.info("Todo deleted successfully", {"todoId": todo_id})
    return ApiResponse(statusCode=200, body={"success": True}, headers={"Content-Type": "application/json"})


async def _update_todo(req: ApiRequest, logger) -> ApiResponse:
    todo_id = req.path_params.get("id")
    existing_todo = await streams.get("todo", "inbox", todo_id) if todo_id else None

    logger.info("Updating todo", {"body": req.body, "todoId": todo_id})

    if not existing_todo:
        logger.error("Todo not found")
        return ApiResponse(statusCode=404, body={"error": "Todo not found"})

    merged = {**existing_todo, **(req.body or {})}
    todo = await streams.set("todo", "inbox", todo_id, merged)

    logger.info("Todo updated successfully", {"todoId": todo_id})
    return ApiResponse(statusCode=200, body=todo, headers={"Content-Type": "application/json"})


async def _create_state(req: ApiRequest, logger) -> ApiResponse:
    logger.info("Creating new todo", {"body": req.body})

    description = req.body.get("description") if req.body else None
    due_date = req.body.get("dueDate") if req.body else None
    todo_id = _generate_todo_id()

    if not description:
        return ApiResponse(statusCode=400, body={"error": "Description is required"})

    new_todo = {
        "id": todo_id,
        "description": description,
        "createdAt": datetime.now(timezone.utc).isoformat(),
        "dueDate": due_date,
        "completedAt": None,
    }
    todo = await state.set("todo", todo_id, new_todo)
    return ApiResponse(statusCode=201, body=todo, headers={"Content-Type": "application/json"})


async def _get_state(req: ApiRequest, logger) -> ApiResponse:
    logger.info("Getting todo", req.path_params)

    todo_id = req.path_params.get("id")
    todo = await state.get("todo", todo_id)
    return ApiResponse(statusCode=200, body=todo, headers={"Content-Type": "application/json"})


async def _error_test(req: ApiRequest, logger) -> ApiResponse:
    raise ValueError("Intentional error for OTEL stacktrace testing")


async def _fetch_example(req: ApiRequest, logger) -> ApiResponse:
    logger.info("Fetching todo from JSONPlaceholder")
    with urllib.request.urlopen("https://jsonplaceholder.typicode.com/todos/1") as response:
        data = json.loads(response.read().decode())
    return ApiResponse(statusCode=200, body=data, headers={"Content-Type": "application/json"})


async def _post_example(req: ApiRequest, logger) -> ApiResponse:
    logger.info("Posting to httpbin", {"body": req.body})
    payload = json.dumps(req.body or {}).encode()
    post_req = urllib.request.Request(
        "https://httpbin.org/post",
        data=payload,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(post_req) as response:
        data = json.loads(response.read().decode())
    return ApiResponse(statusCode=200, body=data, headers={"Content-Type": "application/json"})


def main() -> None:
    from .trigger_types import list_trigger_types_example

    engine_ws_url = os.environ.get("III_URL", "ws://localhost:49134")
    iii = register_worker(
        address=engine_ws_url,
        options=InitOptions(
            worker_name="iii-example",
            otel={"enabled": True, "service_name": "iii-example"},
        ),
    )
    _setup(iii)

    # List all available trigger types with their schemas
    list_trigger_types_example(iii)

if __name__ == "__main__":
    main()
