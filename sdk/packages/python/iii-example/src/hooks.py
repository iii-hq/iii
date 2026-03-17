from typing import Any, Awaitable, Callable

from iii import ApiRequest, ApiResponse, IIIClient, Logger
from iii.iii_types import FunctionInfo


def use_api(
    iii: IIIClient,
    config: dict[str, Any],
    handler: Callable[[ApiRequest[Any], Logger], Awaitable[ApiResponse[Any]]],
) -> None:
    api_path = config["api_path"]
    http_method = config["http_method"]
    function_id = f"api.{http_method.lower()}.{api_path}"
    logger = Logger(service_name=function_id)

    async def wrapped(data: Any) -> dict[str, Any]:
        req = ApiRequest(**data) if isinstance(data, dict) else data
        result = await handler(req, logger)
        return result.model_dump(by_alias=True)

    iii.register_function({"id": function_id}, wrapped)
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


def use_functions_available(iii: IIIClient, callback: Callable[[list[FunctionInfo]], None]) -> Callable[[], None]:
    return iii.on_functions_available(callback)
