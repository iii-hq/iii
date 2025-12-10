"""Step wrapper for registering steps with the bridge."""

import logging
import uuid
from typing import Any, Awaitable, Callable

from iii import get_context

from .bridge import bridge
from .guards import is_api_step, is_cron_step, is_event_step
from .state import StateManager
from .streams import Stream
from .types import ApiRequest, ApiResponse, FlowContext, Step, StepConfig
from .types_stream import StreamConfig

log = logging.getLogger("motia.step")


def _compose_middleware(
    middlewares: list[Callable[[Any, Any, Callable[[], Awaitable[Any]]], Awaitable[Any]]],
) -> Callable[[Any, Any, Callable[[], Awaitable[Any]]], Awaitable[Any]]:
    """Compose multiple middlewares into a single middleware."""

    async def composed(req: Any, ctx: Any, handler: Callable[[], Awaitable[Any]]) -> Any:
        async def create_next(index: int) -> Callable[[], Awaitable[Any]]:
            if index >= len(middlewares):
                return handler

            async def next_handler() -> Any:
                return await middlewares[index](req, ctx, await create_next(index + 1))

            return next_handler

        if not middlewares:
            return await handler()

        first_next = await create_next(1)
        return await middlewares[0](req, ctx, first_next)

    return composed


def step_wrapper(
    config: StepConfig,
    step_path: str,
    handler: Callable[..., Awaitable[Any]],
    streams: dict[str, Stream[Any]] | None = None,
) -> None:
    """Register a step with the bridge."""
    step = Step(file_path=step_path, version="", config=config)
    function_path = f"steps.{step.config.name}"
    state = StateManager()
    streams = streams or {}

    log.info(f"Step registered: {step.config.name}")

    if is_api_step(step):

        async def api_handler(req: dict[str, Any]) -> dict[str, Any]:
            context_data = get_context()

            async def emit(event: Any) -> None:
                await bridge.invoke_function("emit", {"event": event})

            context = FlowContext(
                emit=emit,
                trace_id=str(uuid.uuid4()),
                state=state,
                logger=context_data.logger,
                streams=streams,
            )

            motia_req = ApiRequest(
                path_params=req.get("path_params", {}),
                query_params=req.get("query_params", {}),
                body=req.get("body"),
                headers=req.get("headers", {}),
            )

            middlewares = getattr(step.config, "middleware", None) or []

            if middlewares:
                composed = _compose_middleware(middlewares)
                response: ApiResponse[Any] = await composed(motia_req, context, lambda: handler(motia_req, context))
            else:
                response = await handler(motia_req, context)

            return {
                "status_code": response.status,
                "headers": response.headers,
                "body": response.body,
            }

        bridge.register_function(function_path, api_handler)
    else:

        async def event_handler(req: Any) -> Any:
            context_data = get_context()

            async def emit(event: Any) -> None:
                await bridge.invoke_function("emit", {"event": event})

            context = FlowContext(
                emit=emit,
                trace_id=str(uuid.uuid4()),
                state=state,
                logger=context_data.logger,
                streams=streams,
            )

            return await handler(req, context)

        bridge.register_function(function_path, event_handler)

    # Register triggers
    if is_api_step(step):
        api_path = step.config.path
        if api_path.startswith("/"):
            api_path = api_path[1:]

        bridge.register_trigger(
            trigger_type="api",
            function_path=function_path,
            config={"api_path": api_path, "http_method": step.config.method},
        )
    elif is_event_step(step):
        bridge.register_trigger(
            trigger_type="event",
            function_path=function_path,
            config={"topic": step.config.subscribes[0]},
        )
    elif is_cron_step(step):
        bridge.register_trigger(
            trigger_type="cron",
            function_path=function_path,
            config={"cron": step.config.cron},
        )


def stream_wrapper(config: StreamConfig, stream_path: str) -> Stream[Any]:
    """Create and register a stream."""
    log.info(f"Stream registered: {config.name}")
    return Stream(config.name)
