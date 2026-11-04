import uuid
from typing import Any, Awaitable, Callable

from iii import IIIClient
from iii.engine import EngineSources
from iii_helpers.http import HttpRequest, HttpResponse
from iii_helpers.observability import Logger


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

    iii.register_function(
        function_id, wrapped, description=config.get("description"), metadata=config.get("metadata")
    )
    # the route: an observer of the http worker's function
    iii.observe(
        {"function_id": "http::on-request", "args": {"method": http_method, "path": f"/{api_path}"}},
        function_id,
    )


def use_functions_available(
    iii: IIIClient, callback: Callable[[list[dict[str, Any]]], None]
) -> Callable[[], None]:
    handler_fn_id = f"iii_example.functions_available_listener.{uuid.uuid4()}"

    async def handler(event: dict[str, Any]) -> None:
        # the rows of `engine::functions::list`, as plain dicts
        callback(event.get("functions", []))

    fn_ref = iii.register_function(handler_fn_id, handler)
    observer = iii.observe({"function_id": EngineSources.FUNCTIONS_CHANGED, "deliver": "input"}, handler_fn_id)

    def unsubscribe() -> None:
        observer.dispose()
        fn_ref.unregister()

    return unsubscribe
