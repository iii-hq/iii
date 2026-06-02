import uuid
from typing import Any, Awaitable, Callable

from iii import HttpRequest, HttpResponse, IIIClient
from iii_observability import Logger


def use_api(
    iii: IIIClient,
    config: dict[str, Any],
    handler: Callable[[HttpRequest[Any], Logger], Awaitable[HttpResponse[Any]]],
) -> None:
    api_path = config["api_path"]
    http_method = config["http_method"]
    function_id = f"api.{http_method.lower()}.{api_path}"
    logger = Logger(service_name=function_id)

    async def wrapped(data: HttpRequest) -> dict[str, Any]:
        req = HttpRequest(**data) if isinstance(data, dict) else data
        result = await handler(req, logger)
        return result.model_dump(by_alias=True)

    iii.register_function(function_id, wrapped)
    iii.register_trigger(
        {
            "type": "http",
            "function_id": function_id,
            "config": {
                "api_path": api_path,
                "http_method": http_method,
                "description": config.get("description"),
                "metadata": config.get("metadata"),
            },
        }
    )


def use_functions_available(
    iii: IIIClient, callback: Callable[[list[dict[str, Any]]], None]
) -> Callable[[], None]:
    handler_fn_id = f"iii_example.functions_available_listener.{uuid.uuid4()}"

    async def handler(data: dict[str, Any]) -> None:
        # The SDK no longer ships a `FunctionInfo`/`FunctionSummary` class —
        # forward the raw dict rows produced by `engine::functions::list`.
        callback(data.get("functions", []))

    fn_ref = iii.register_function(handler_fn_id, handler)
    trigger_guard = iii.register_trigger(
        {
            "type": "engine::functions-available",
            "function_id": handler_fn_id,
            "config": {},
        }
    )

    def unsubscribe() -> None:
        trigger_guard.unregister()
        fn_ref.unregister()

    return unsubscribe
