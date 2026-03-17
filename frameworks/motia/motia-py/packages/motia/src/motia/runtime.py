"""Runtime manager for Motia framework."""

import inspect
import json
import logging
import uuid
from typing import Any, Awaitable, Callable

from iii import http as iii_http
from iii.telemetry import current_trace_id
from iii.types import HttpRequest as IIIHttpRequest
from iii.types import HttpResponse as IIIHttpResponse
from pydantic import BaseModel
from pydantic import ValidationError as PydanticValidationError

from .iii import get_instance
from .schema_utils import schema_to_json_schema
from .step import StepDefinition
from .streams import Stream
from .tracing import instrument_bridge, record_exception, set_span_ok, step_span
from .types import (
    ApiRequest,
    ApiTrigger,
    CronTrigger,
    FlowContext,
    MotiaHttpArgs,
    MotiaHttpRequest,
    MotiaHttpResponse,
    QueueTrigger,
    StateTrigger,
    StepConfig,
    StreamTrigger,
    TriggerConfig,
    TriggerInfo,
)
from .types_stream import StreamAuthInput, StreamAuthResult, StreamConfig, StreamSubscription
from .validator import validate_step

log = logging.getLogger("motia.runtime")
CONDITION_PATH_KEY = "condition_function_id"


def _compose_middleware(
    middlewares: list[Callable[..., Any]],
) -> Callable[..., Any]:
    """Compose multiple middlewares into a single middleware."""

    async def composed(req: Any, ctx: Any, handler: Callable[..., Any]) -> Any:
        composed_handler = handler
        for middleware in reversed(middlewares):

            def make_next(
                m: Callable[..., Any] = middleware,
                p: Callable[..., Any] = composed_handler,
            ) -> Callable[..., Any]:
                async def next_handler() -> Any:
                    result = m(req, ctx, p)
                    if inspect.iscoroutine(result):
                        return await result
                    return result

                return next_handler

            composed_handler = make_next()
        result = composed_handler()
        if inspect.iscoroutine(result):
            return await result
        return result

    return composed


def _jsonable_value(value: Any) -> tuple[bool, Any | None]:
    """Check if a value is JSON-serializable."""
    if value is None:
        return True, None
    if callable(value):
        return False, None
    json_schema = schema_to_json_schema(value)
    if json_schema is not None:
        return True, json_schema
    try:
        json.dumps(value)
    except TypeError:
        return False, None
    return True, value


def _sanitize_trigger_metadata(trigger: TriggerConfig) -> dict[str, Any]:
    """Convert trigger to serializable metadata dict."""
    data = trigger.model_dump(by_alias=True, exclude_none=True)
    data.pop("condition", None)
    data.pop("middleware", None)

    if isinstance(trigger, QueueTrigger):
        ok, input_value = _jsonable_value(trigger.input)
        if ok and input_value is not None:
            data["input"] = input_value
        else:
            data.pop("input", None)

    if isinstance(trigger, ApiTrigger):
        ok, body_value = _jsonable_value(trigger.body_schema)
        if ok and body_value is not None:
            data["bodySchema"] = body_value
        else:
            data.pop("bodySchema", None)

        if trigger.response_schema:
            response_schema: dict[int, Any] = {}
            for status_code, schema in trigger.response_schema.items():
                ok, schema_value = _jsonable_value(schema)
                if ok and schema_value is not None:
                    response_schema[status_code] = schema_value
            if response_schema:
                data["responseSchema"] = response_schema
            else:
                data.pop("responseSchema", None)

    return data


def _get_trigger_suffix(trigger: "TriggerConfig") -> str:
    """Return a descriptive suffix for a trigger based on its type and key properties."""
    if isinstance(trigger, ApiTrigger):
        return f"http({trigger.method} {trigger.path})"
    if isinstance(trigger, CronTrigger):
        return f"cron({trigger.expression})"
    if isinstance(trigger, QueueTrigger):
        return f"queue({trigger.topic})"
    if isinstance(trigger, StreamTrigger):
        return f"stream({trigger.stream_name})"
    if isinstance(trigger, StateTrigger):
        return "state"
    return "unknown"


def _validate_input_schema(schema: Any, value: Any, label: str) -> Any:
    """Validate input with a Pydantic model or JSON Schema if available."""
    if schema is None:
        return value
    if isinstance(schema, type):
        if issubclass(schema, BaseModel):
            try:
                return schema.model_validate(value)
            except PydanticValidationError as exc:
                log.error(
                    "Pydantic input validation failed for label=%s schema=%r: %s",
                    label,
                    schema,
                    exc,
                    exc_info=True,
                )
                raise

    json_schema = schema_to_json_schema(schema)
    if json_schema:
        try:
            import jsonschema  # type: ignore
        except ImportError:
            log.warning(
                "jsonschema is not installed; skipping JSON Schema validation for label=%s json_schema=%r",
                label,
                json_schema,
            )
            return value

        try:
            jsonschema.validate(instance=value, schema=json_schema)
        except jsonschema.ValidationError as exc:
            log.error(
                "JSON Schema validation failed for label=%s json_schema=%r: %s",
                label,
                json_schema,
                exc,
                exc_info=True,
            )
            raise

    return value


def _flow_context(
    trigger: TriggerInfo,
    input_data: Any = None,
) -> FlowContext[Any]:
    """Create a FlowContext for a handler invocation."""
    trace_id = current_trace_id() or str(uuid.uuid4())

    return FlowContext(
        trace_id=trace_id,
        trigger=trigger,
        input_value=input_data,
    )


class Motia:
    """Centralized runtime manager for Motia applications."""

    def __init__(self) -> None:
        self.streams: dict[str, Stream[Any]] = {}
        self._stream_configs: dict[str, StreamConfig] = {}
        self._authenticate: Callable[..., StreamAuthResult | bool | Awaitable[StreamAuthResult | bool]] | None = None

    def add_stream(self, config: StreamConfig, file_path: str) -> Stream[Any]:
        """Add a stream to the runtime."""
        stream: Stream[Any] = Stream(config)
        self.streams[config.name] = stream
        self._stream_configs[config.name] = config
        log.info(f"Stream added: {config.name} from {file_path}")
        return stream

    def set_authenticate(
        self,
        handler: Callable[..., StreamAuthResult | bool | Awaitable[StreamAuthResult | bool]],
    ) -> None:
        """Set the global stream authentication handler."""
        self._authenticate = handler

    def add_step(
        self,
        config: Any,
        step_path: str,
        handler: Callable[..., Any] | None = None,
        file_path: str | None = None,
    ) -> None:
        """Add a step to the runtime and register with III engine.

        Accepts either:
        - config as StepDefinition (from step() or multi_trigger_step()): handler extracted automatically
        - config as StepConfig/dict + separate handler argument
        """
        if isinstance(config, StepDefinition):
            if handler is None:
                handler = config.handler
            config = config.config

        if handler is None:
            raise ValueError("handler is required - provide it directly or use step()/multi_trigger_step()")

        if not isinstance(config, StepConfig):
            config = StepConfig.model_validate(config)

        errors = validate_step(config)
        if errors:
            raise ValueError(f"Invalid step config for {config.name}: {', '.join(errors)}")

        instrument_bridge(get_instance())
        actual_file_path = file_path or step_path
        raw_metadata = config.model_dump(by_alias=True, exclude_none=True)
        if "triggers" in raw_metadata:
            raw_metadata["triggers"] = [_sanitize_trigger_metadata(t) for t in config.triggers]
        metadata = {**raw_metadata, "filePath": actual_file_path}

        log.info(f"Step registered: {config.name}")

        seen_suffixes: set[str] = set()

        for index, trigger in enumerate(config.triggers):
            trigger_suffix = _get_trigger_suffix(trigger)
            if trigger_suffix in seen_suffixes:
                trigger_suffix = f"{trigger_suffix}::{index}"
            seen_suffixes.add(trigger_suffix)
            function_id = f"steps::{config.name}::trigger::{trigger_suffix}"

            if isinstance(trigger, ApiTrigger):
                self._register_api_trigger(config, trigger, handler, function_id, index, metadata)
            elif isinstance(trigger, QueueTrigger):
                self._register_queue_trigger(config, trigger, handler, function_id, index, metadata)
            elif isinstance(trigger, CronTrigger):
                self._register_cron_trigger(config, trigger, handler, function_id, index, metadata)
            elif isinstance(trigger, StateTrigger):
                self._register_state_trigger(config, trigger, handler, function_id, index, metadata)
            elif isinstance(trigger, StreamTrigger):
                self._register_stream_trigger(config, trigger, handler, function_id, index, metadata)

    def _register_api_trigger(
        self,
        config: StepConfig,
        trigger: ApiTrigger,
        handler: Callable[..., Any],
        function_id: str,
        index: int,
        metadata: dict[str, Any],
    ) -> None:
        @iii_http  # type: ignore[untyped-decorator]
        async def api_handler(req: IIIHttpRequest, res: IIIHttpResponse) -> Any:
            with step_span(
                config.name,
                "http",
                **{"http.method": req.method, "http.route": trigger.path},
            ) as span:
                try:
                    trigger_info = TriggerInfo(type="http", index=index, method=trigger.method, path=trigger.path)

                    stream_response = MotiaHttpResponse(res.writer)
                    http_request: MotiaHttpRequest[Any] = MotiaHttpRequest(
                        path_params=req.path_params,
                        query_params=req.query_params,
                        body=req.body,
                        headers=req.headers,
                        method=req.method,
                        request_body=req.request_body,
                    )
                    motia_request: MotiaHttpArgs[Any] = MotiaHttpArgs(
                        request=http_request,
                        response=stream_response,
                    )

                    context = _flow_context(trigger_info, motia_request)
                    middlewares = trigger.middleware or []

                    if middlewares:
                        composed = _compose_middleware(middlewares)
                        result = composed(motia_request, context, lambda: handler(motia_request, context))
                        if inspect.iscoroutine(result):
                            result = await result
                    else:
                        result = handler(motia_request, context)
                        if inspect.iscoroutine(result):
                            result = await result

                    if result is not None and hasattr(result, "model_dump"):
                        result = result.model_dump()
                        if "status" in result and "status_code" not in result:
                            result["status_code"] = result.pop("status")

                    if result is not None and isinstance(result, dict):
                        status_code = int(result.get("status_code", result.get("status", 200)))
                        headers_out = result.get("headers") or {}
                        body_out = result.get("body")

                        await stream_response.status(status_code)
                        if headers_out:
                            await stream_response.headers(headers_out)
                        if body_out is not None and stream_response.writer is not None:
                            payload = (
                                body_out
                                if isinstance(body_out, (bytes, bytearray))
                                else json.dumps(body_out).encode("utf-8")
                            )
                            stream_response.writer.stream.write(payload)
                        stream_response.close()

                    set_span_ok(span)
                    if isinstance(result, dict):
                        return result
                    return None
                except Exception as exc:
                    record_exception(span, exc)
                    raise

        get_instance().register_function({"id": function_id}, api_handler)

        api_path = trigger.path.lstrip("/")
        trigger_config: dict[str, Any] = {
            "api_path": api_path,
            "http_method": trigger.method,
            "metadata": metadata,
        }

        if trigger.condition:
            condition_path = f"{function_id}::conditions::{index}"
            self._register_condition(trigger, condition_path, "http", index)
            trigger_config[CONDITION_PATH_KEY] = condition_path

        get_instance().register_trigger({"type": "http", "function_id": function_id, "config": trigger_config})

    def _register_queue_trigger(
        self,
        config: StepConfig,
        trigger: QueueTrigger,
        handler: Callable[..., Any],
        function_id: str,
        index: int,
        metadata: dict[str, Any],
    ) -> None:
        async def queue_handler(req: Any) -> Any:
            with step_span(config.name, "queue") as span:
                try:
                    trigger_info = TriggerInfo(type="queue", index=index)
                    input_data = req
                    if trigger.input:
                        input_data = _validate_input_schema(trigger.input, input_data, f"queue:{config.name}")
                    context = _flow_context(trigger_info, input_data)
                    result = handler(input_data, context)
                    if inspect.iscoroutine(result):
                        result = await result
                    set_span_ok(span)
                    return result
                except Exception as exc:
                    record_exception(span, exc)
                    raise

        get_instance().register_function({"id": function_id}, queue_handler)

        trigger_config: dict[str, Any] = {
            "topic": trigger.topic,
            "metadata": {**metadata},
        }
        if trigger.config:
            trigger_config["queue_config"] = trigger.config.model_dump(by_alias=True, exclude_none=True)

        if trigger.condition:
            condition_path = f"{function_id}::conditions::{index}"
            self._register_condition(trigger, condition_path, "queue", index)
            trigger_config[CONDITION_PATH_KEY] = condition_path

        get_instance().register_trigger({"type": "queue", "function_id": function_id, "config": trigger_config})

    def _register_cron_trigger(
        self,
        config: StepConfig,
        trigger: CronTrigger,
        handler: Callable[..., Any],
        function_id: str,
        index: int,
        metadata: dict[str, Any],
    ) -> None:
        async def cron_handler(_req: Any) -> Any:
            with step_span(config.name, "cron") as span:
                try:
                    trigger_info = TriggerInfo(type="cron", index=index)
                    context = _flow_context(trigger_info)
                    result = handler(None, context)
                    if inspect.iscoroutine(result):
                        result = await result
                    set_span_ok(span)
                    return result
                except Exception as exc:
                    record_exception(span, exc)
                    raise

        get_instance().register_function({"id": function_id}, cron_handler)

        trigger_config: dict[str, Any] = {
            "expression": trigger.expression,
            "metadata": metadata,
        }

        if trigger.condition:
            condition_path = f"{function_id}::conditions::{index}"
            self._register_condition(trigger, condition_path, "cron", index)
            trigger_config[CONDITION_PATH_KEY] = condition_path

        get_instance().register_trigger({"type": "cron", "function_id": function_id, "config": trigger_config})

    def _register_state_trigger(
        self,
        config: StepConfig,
        trigger: StateTrigger,
        handler: Callable[..., Any],
        function_id: str,
        index: int,
        metadata: dict[str, Any],
    ) -> None:
        async def state_handler(req: Any) -> Any:
            with step_span(config.name, "state") as span:
                try:
                    trigger_info = TriggerInfo(type="state", index=index)
                    context = _flow_context(trigger_info, req)
                    result = handler(req, context)
                    if inspect.iscoroutine(result):
                        result = await result
                    set_span_ok(span)
                    return result
                except Exception as exc:
                    record_exception(span, exc)
                    raise

        get_instance().register_function({"id": function_id}, state_handler)

        trigger_config: dict[str, Any] = {"metadata": metadata}

        if trigger.condition:
            condition_path = f"{function_id}::conditions::{index}"
            self._register_condition(trigger, condition_path, "state", index)
            trigger_config["condition_function_id"] = condition_path

        get_instance().register_trigger({"type": "state", "function_id": function_id, "config": trigger_config})

    def _register_stream_trigger(
        self,
        config: StepConfig,
        trigger: StreamTrigger,
        handler: Callable[..., Any],
        function_id: str,
        index: int,
        metadata: dict[str, Any],
    ) -> None:
        async def stream_handler(req: Any) -> Any:
            with step_span(config.name, "stream") as span:
                try:
                    trigger_info = TriggerInfo(type="stream", index=index)
                    context = _flow_context(trigger_info, req)
                    result = handler(req, context)
                    if inspect.iscoroutine(result):
                        result = await result
                    set_span_ok(span)
                    return result
                except Exception as exc:
                    record_exception(span, exc)
                    raise

        get_instance().register_function({"id": function_id}, stream_handler)

        trigger_config: dict[str, Any] = {
            "metadata": metadata,
            "stream_name": trigger.stream_name,
        }
        if trigger.group_id:
            trigger_config["group_id"] = trigger.group_id
        if trigger.item_id:
            trigger_config["item_id"] = trigger.item_id

        if trigger.condition:
            condition_path = f"{function_id}::conditions::{index}"
            self._register_condition(trigger, condition_path, "stream", index)
            trigger_config["condition_function_id"] = condition_path

        get_instance().register_trigger({"type": "stream", "function_id": function_id, "config": trigger_config})

    def _register_condition(
        self,
        trigger: TriggerConfig,
        condition_path: str,
        trigger_type: str,
        index: int,
    ) -> None:
        """Register a condition function for a trigger."""
        condition = trigger.condition
        if condition is None:
            return

        async def condition_handler(input_data: Any) -> bool:
            trigger_info = TriggerInfo(type=trigger_type, index=index)

            if isinstance(trigger, ApiTrigger):
                motia_input: ApiRequest[Any] = ApiRequest(
                    path_params=input_data.get("path_params", {}) if isinstance(input_data, dict) else {},
                    query_params=input_data.get("query_params", {}) if isinstance(input_data, dict) else {},
                    body=input_data.get("body") if isinstance(input_data, dict) else input_data,
                    headers=input_data.get("headers", {}) if isinstance(input_data, dict) else {},
                )
                context = _flow_context(trigger_info, motia_input)
                result = condition(motia_input, context)
            else:
                context = _flow_context(trigger_info, input_data)
                result = condition(input_data, context)

            if inspect.iscoroutine(result):
                return bool(await result)
            return bool(result)

        get_instance().register_function({"id": condition_path}, condition_handler)

    def initialize(self) -> None:
        """Initialize the runtime and register bridge functions."""

        if self._authenticate:
            get_instance().register_function({"id": "motia::stream::authenticate"}, self._handle_authenticate)
            log.debug("Registered stream authentication handler")

        has_join = any(config.on_join for config in self._stream_configs.values())
        has_leave = any(config.on_leave for config in self._stream_configs.values())

        from .setup_step_endpoint import setup_step_endpoint

        setup_step_endpoint(get_instance())

        if has_join:
            function_id = "motia::stream::join"

            async def join_handler(req: dict[str, Any]) -> Any:
                stream_name = req.get("stream_name", "")
                group_id = req.get("group_id", "")
                client_id = req.get("id")
                auth_context = req.get("context")

                config = self._stream_configs.get(stream_name)
                if config and config.on_join:
                    trigger_info = TriggerInfo(type="queue")
                    context = _flow_context(trigger_info)
                    subscription = StreamSubscription(group_id=group_id, id=client_id)
                    result = config.on_join(subscription, context, auth_context)
                    if inspect.iscoroutine(result):
                        result = await result
                    return result

            get_instance().register_function({"id": function_id}, join_handler)
            get_instance().register_trigger({"type": "stream:join", "function_id": function_id, "config": {}})
            log.debug("Registered stream join handler")

        if has_leave:
            function_id = "motia::stream::leave"

            async def leave_handler(req: dict[str, Any]) -> Any:
                stream_name = req.get("stream_name", "")
                group_id = req.get("group_id", "")
                client_id = req.get("id")
                auth_context = req.get("context")

                config = self._stream_configs.get(stream_name)
                if config and config.on_leave:
                    trigger_info = TriggerInfo(type="queue")
                    context = _flow_context(trigger_info)
                    subscription = StreamSubscription(group_id=group_id, id=client_id)
                    result = config.on_leave(subscription, context, auth_context)
                    if inspect.iscoroutine(result):
                        await result

            get_instance().register_function({"id": function_id}, leave_handler)
            get_instance().register_trigger({"type": "stream:leave", "function_id": function_id, "config": {}})
            log.debug("Registered stream leave handler")

    async def _handle_authenticate(self, data: dict[str, Any]) -> dict[str, Any]:
        """Handle stream authentication."""
        if not self._authenticate:
            return {"authorized": True}

        input_data = StreamAuthInput(
            headers=data.get("headers", {}),
            path=data.get("path", ""),
            query_params=data.get("query_params", {}),
            addr=data.get("addr", ""),
        )

        trigger_info = TriggerInfo(type="queue")
        context = _flow_context(trigger_info, input_data)
        result = self._authenticate(input_data, context)
        if inspect.iscoroutine(result):
            result = await result

        if isinstance(result, bool):
            return {"authorized": result}
        if isinstance(result, StreamAuthResult):
            return result.model_dump()
        return {"authorized": bool(result)}
