"""Step wrapper for registering steps with the bridge."""

import asyncio
import inspect
import logging
import uuid
from typing import Any, Awaitable, Callable

from iii import get_context

from .bridge import bridge
from .state import StateManager
from .streams import Stream
from .types import (
    ApiRequest,
    ApiResponse,
    ApiTrigger,
    CronTrigger,
    EventTrigger,
    FlowContext,
    Step,
    StepConfig,
    TriggerCondition,
    TriggerConfig,
    TriggerInput,
    TriggerMetadata,
)
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




def _trigger_to_engine_config(trigger: TriggerConfig) -> dict[str, Any]:
    """Convert trigger config to engine config format."""
    if isinstance(trigger, EventTrigger):
        return {"topic": trigger.subscribes[0] if trigger.subscribes else ""}
    elif isinstance(trigger, ApiTrigger):
        api_path = trigger.path
        if api_path.startswith("/"):
            api_path = api_path[1:]
        return {"api_path": api_path, "http_method": trigger.method}
    elif isinstance(trigger, CronTrigger):
        return {"expression": trigger.expression}
    return {}


def step_wrapper(
    config: StepConfig,
    step_path: str,
    handler: Callable[..., Awaitable[Any]],
    streams: dict[str, Stream[Any]] | None = None,
) -> None:
    """Register a step with the bridge."""
    step = Step(file_path=step_path, version="", config=config)
    state = StateManager()
    streams = streams or {}

    log.info(f"Step registered: {step.config.name}")

    for trigger_index, trigger in enumerate(config.triggers):
        function_path = f"steps.{step.config.name}:trigger:{trigger_index}"
        trigger_info = {"type": trigger.type, "index": trigger_index}
        
        is_api_trigger = isinstance(trigger, ApiTrigger)

        if is_api_trigger:

            async def api_handler(
                req: dict[str, Any],
                _trigger=trigger,
                _trigger_index=trigger_index,
            ) -> dict[str, Any]:
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

                trigger_input = TriggerInput(
                    trigger=TriggerMetadata(
                        type="api",
                        index=_trigger_index,
                        path=_trigger.path,
                        method=_trigger.method,
                    ),
                    request=motia_req,
                    data=motia_req.body,
                )

                middlewares = getattr(step.config, "middleware", None) or []

                if middlewares:
                    composed = _compose_middleware(middlewares)
                    response: ApiResponse[Any] = await composed(
                        trigger_input, context, lambda: handler(trigger_input, context)
                    )
                else:
                    response = await handler(trigger_input, context)

                return {
                    "status_code": response.status,
                    "headers": response.headers,
                    "body": response.body,
                }

            bridge.register_function(function_path, api_handler)
        else:

            async def event_handler(
                req: Any, _trigger=trigger, _trigger_index=trigger_index
            ) -> Any:
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

                if isinstance(_trigger, EventTrigger):
                    trigger_input = TriggerInput(
                        trigger=TriggerMetadata(
                            type="event",
                            index=_trigger_index,
                            topic=_trigger.subscribes[0] if _trigger.subscribes else None,
                        ),
                        request=None,
                        data=req,
                    )
                elif isinstance(_trigger, CronTrigger):
                    trigger_input = TriggerInput(
                        trigger=TriggerMetadata(
                            type="cron",
                            index=_trigger_index,
                            expression=_trigger.expression,
                        ),
                        request=None,
                        data={},
                    )
                else:
                    trigger_input = TriggerInput(
                        trigger=TriggerMetadata(
                            type="event",
                            index=_trigger_index,
                        ),
                        request=None,
                        data=req,
                    )

                return await handler(trigger_input, context)

            bridge.register_function(function_path, event_handler)

        engine_config = _trigger_to_engine_config(trigger)
        
        if trigger.condition:
            condition_function_path = f"{function_path}.conditions:{trigger_index}"
            engine_config["_condition_path"] = condition_function_path
            
            async def condition_handler(
                input_data: Any, _trigger=trigger, _trigger_index=trigger_index
            ) -> bool:
                result = _trigger.condition(input_data, {}, {"type": _trigger.type, "index": _trigger_index})
                if inspect.iscoroutine(result):
                    result = await result
                return result
            
            bridge.register_function(condition_function_path, condition_handler)
        
        bridge.register_trigger(
            trigger_type=trigger.type,
            function_path=function_path,
            config=engine_config,
        )


def stream_wrapper(config: StreamConfig, stream_path: str) -> Stream[Any]:
    """Create and register a stream."""
    log.info(f"Stream registered: {config.name}")
    return Stream(config.name)
