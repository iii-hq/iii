"""Bridge implementation for WebSocket communication with the III Engine."""

import asyncio
import json
import logging
import uuid
from typing import Any, Awaitable, Callable

import websockets
from websockets.asyncio.client import ClientConnection

from .bridge_types import (
    InvocationResultMessage,
    InvokeFunctionMessage,
    MessageType,
    RegisterFunctionMessage,
    RegisterServiceMessage,
    RegisterTriggerMessage,
    RegisterTriggerTypeMessage,
    TriggerConfig as BridgeTriggerConfig,
    UnregisterTriggerMessage,
    UnregisterTriggerTypeMessage,
)
from .context import Context, get_context, with_context
from .logger import Logger
from .triggers import Trigger, TriggerConfig, TriggerHandler
from .types import RemoteFunctionData, RemoteTriggerTypeData

RemoteFunctionHandler = Callable[[Any], Awaitable[Any]]

log = logging.getLogger("iii.bridge")


class Bridge:
    """WebSocket bridge for communication with the III Engine."""

    def __init__(self, address: str) -> None:
        self._address = address
        self._ws: ClientConnection | None = None
        self._functions: dict[str, RemoteFunctionData] = {}
        self._services: dict[str, RegisterServiceMessage] = {}
        self._pending: dict[str, asyncio.Future[Any]] = {}
        self._triggers: dict[str, RegisterTriggerMessage] = {}
        self._trigger_types: dict[str, RemoteTriggerTypeData] = {}
        self._queue: list[dict[str, Any]] = []
        self._reconnect_task: asyncio.Task[None] | None = None
        self._running = False
        self._receiver_task: asyncio.Task[None] | None = None

    # Connection management

    async def connect(self) -> None:
        """Connect to the WebSocket server."""
        self._running = True
        await self._do_connect()

    async def disconnect(self) -> None:
        """Disconnect from the WebSocket server."""
        self._running = False

        for task in [self._reconnect_task, self._receiver_task]:
            if task:
                task.cancel()
                try:
                    await task
                except asyncio.CancelledError:
                    pass

        if self._ws:
            await self._ws.close()
            self._ws = None

    async def _do_connect(self) -> None:
        try:
            log.debug(f"Connecting to {self._address}")
            self._ws = await websockets.connect(self._address)
            log.info(f"Connected to {self._address}")
            await self._on_connected()
        except Exception as e:
            log.warning(f"Connection failed: {e}")
            if self._running:
                self._schedule_reconnect()

    def _schedule_reconnect(self) -> None:
        if not self._reconnect_task or self._reconnect_task.done():
            self._reconnect_task = asyncio.create_task(self._reconnect_loop())

    async def _reconnect_loop(self) -> None:
        while self._running and not self._ws:
            await asyncio.sleep(2)
            await self._do_connect()

    async def _on_connected(self) -> None:
        # Re-register all
        for data in self._trigger_types.values():
            await self._send(data.message)
        for svc in self._services.values():
            await self._send(svc)
        for data in self._functions.values():
            await self._send(data.message)
        for trigger in self._triggers.values():
            await self._send(trigger)

        # Flush queue
        while self._queue and self._ws:
            await self._ws.send(json.dumps(self._queue.pop(0)))

        self._receiver_task = asyncio.create_task(self._receive_loop())

    async def _receive_loop(self) -> None:
        if not self._ws:
            return
        try:
            async for msg in self._ws:
                await self._handle_message(msg)
        except websockets.ConnectionClosed:
            log.debug("Connection closed")
            self._ws = None
            if self._running:
                self._schedule_reconnect()

    # Message handling

    def _to_dict(self, msg: Any) -> dict[str, Any]:
        if isinstance(msg, dict):
            return msg
        if hasattr(msg, "model_dump"):
            data = msg.model_dump(by_alias=True, exclude_none=True)
            if "type" in data and hasattr(data["type"], "value"):
                data["type"] = data["type"].value
            return data
        return {"data": msg}

    async def _send(self, msg: Any) -> None:
        data = self._to_dict(msg)
        if self._ws and self._ws.state.name == "OPEN":
            log.debug(f"Send: {json.dumps(data)[:200]}")
            await self._ws.send(json.dumps(data))
        else:
            self._queue.append(data)

    def _enqueue(self, msg: Any) -> None:
        self._queue.append(self._to_dict(msg))

    async def _handle_message(self, raw: str | bytes) -> None:
        data = json.loads(raw if isinstance(raw, str) else raw.decode())
        msg_type = data.get("type")
        log.debug(f"Recv: {msg_type}")

        if msg_type == MessageType.INVOCATION_RESULT.value:
            self._handle_result(
                data.get("invocation_id", ""),
                data.get("result"),
                data.get("error"),
            )
        elif msg_type == MessageType.INVOKE_FUNCTION.value:
            asyncio.create_task(
                self._handle_invoke(
                    data.get("invocation_id"),
                    data.get("function_path", ""),
                    data.get("data"),
                )
            )
        elif msg_type == MessageType.REGISTER_TRIGGER.value:
            asyncio.create_task(self._handle_trigger_registration(data))

    def _handle_result(self, invocation_id: str, result: Any, error: Any) -> None:
        future = self._pending.pop(invocation_id, None)
        if not future:
            log.debug(f"No pending invocation: {invocation_id}")
            return

        if error:
            future.set_exception(Exception(str(error)))
        else:
            future.set_result(result)

    async def _handle_invoke(self, invocation_id: str | None, path: str, data: Any) -> None:
        func = self._functions.get(path)

        if not func:
            log.warning(f"Function not found: {path}")
            if invocation_id:
                await self._send(
                    InvocationResultMessage(
                        invocation_id=invocation_id,
                        function_path=path,
                        error={"code": "function_not_found", "message": f"Function '{path}' not found"},
                    )
                )
            return

        if not invocation_id:
            asyncio.create_task(func.handler(data))
            return

        try:
            result = await func.handler(data)
            await self._send(
                InvocationResultMessage(
                    invocation_id=invocation_id,
                    function_path=path,
                    result=result,
                )
            )
        except Exception as e:
            log.exception(f"Error in handler {path}")
            await self._send(
                InvocationResultMessage(
                    invocation_id=invocation_id,
                    function_path=path,
                    error={"code": "invocation_failed", "message": str(e)},
                )
            )

    async def _handle_trigger_registration(self, data: dict[str, Any]) -> None:
        trigger_id = data.get("id", "")
        function_path = data.get("function_path", "")
        triggers = data.get("triggers", [])

        for trigger_config in triggers:
            trigger_type_id = trigger_config.get("trigger_type")
            handler_data = self._trigger_types.get(trigger_type_id) if trigger_type_id else None
            config = trigger_config.get("config")

            result_base = {
                "type": MessageType.TRIGGER_REGISTRATION_RESULT.value,
                "id": trigger_id,
                "trigger_type": trigger_type_id,
                "function_path": function_path,
            }

            if not handler_data:
                continue

            try:
                await handler_data.handler.register_trigger(
                    TriggerConfig(id=trigger_id, function_path=function_path, config=config)
                )
                await self._send(result_base)
            except Exception as e:
                log.exception(f"Error registering trigger {trigger_id}")
                await self._send({**result_base, "error": {"code": "trigger_registration_failed", "message": str(e)}})

    # Public API

    def register_trigger_type(self, id: str, description: str, handler: TriggerHandler[Any]) -> None:
        msg = RegisterTriggerTypeMessage(id=id, description=description)
        self._enqueue(msg)
        self._trigger_types[id] = RemoteTriggerTypeData(message=msg, handler=handler)

    def unregister_trigger_type(self, id: str) -> None:
        self._enqueue(UnregisterTriggerTypeMessage(id=id))
        self._trigger_types.pop(id, None)

    def register_trigger(
        self,
        function_path: str,
        triggers: list[dict[str, Any]],
    ) -> Trigger:
        trigger_id = str(uuid.uuid4())

        condition_function_paths: list[str] = []
        trigger_configs = []
        for index, t in enumerate(triggers):
            trigger_type = t.get("trigger_type", "")
            config = t.get("config", {})
            condition = t.get("condition")

            if condition:
                condition_function_path = f"{function_path}.conditions:{index}"
                config = {**config, "_condition_path": condition_function_path}
                condition_function_paths.append(condition_function_path)
                
                async def condition_handler(input_data: Any) -> bool:
                    context = get_context()
                    return await condition(input_data, context)
                
                self.register_function(condition_function_path, condition_handler)

            trigger_configs.append(
                BridgeTriggerConfig(
                    trigger_type=trigger_type,
                    config=config,
                )
            )

        msg = RegisterTriggerMessage(
            id=trigger_id,
            function_path=function_path,
            triggers=trigger_configs,
        )
        self._enqueue(msg)
        self._triggers[trigger_id] = msg

        def unregister() -> None:
            self._enqueue(UnregisterTriggerMessage(id=trigger_id))
            self._triggers.pop(trigger_id, None)
            
            for condition_path in condition_function_paths:
                self._functions.pop(condition_path, None)

        return Trigger(unregister)

    def register_function(self, path: str, handler: RemoteFunctionHandler, description: str | None = None) -> None:
        msg = RegisterFunctionMessage(function_path=path, description=description)
        self._enqueue(msg)

        async def wrapped(input_data: Any) -> Any:
            trace_id = str(uuid.uuid4())
            logger = Logger(
                lambda fn, params: self.invoke_function_async(fn, params),
                trace_id,
                path,
            )
            ctx = Context(logger=logger)
            return await with_context(lambda _: handler(input_data), ctx)

        self._functions[path] = RemoteFunctionData(message=msg, handler=wrapped)

    def function(self, path: str, description: str | None = None):
        """Decorator to register a function."""

        def decorator(handler: RemoteFunctionHandler) -> RemoteFunctionHandler:
            self.register_function(path, handler, description)
            return handler

        return decorator

    def register_service(self, id: str, description: str | None = None, parent_id: str | None = None) -> None:
        msg = RegisterServiceMessage(id=id, description=description, parent_service_id=parent_id)
        self._enqueue(msg)
        self._services[id] = msg

    async def invoke_function(self, path: str, data: Any, timeout: float = 30.0) -> Any:
        """Invoke a remote function and wait for the result."""
        invocation_id = str(uuid.uuid4())
        future: asyncio.Future[Any] = asyncio.get_running_loop().create_future()

        self._pending[invocation_id] = future

        await self._send(
            InvokeFunctionMessage(
                function_path=path,
                data=data,
                invocation_id=invocation_id,
            )
        )

        try:
            return await asyncio.wait_for(future, timeout=timeout)
        except asyncio.TimeoutError:
            self._pending.pop(invocation_id, None)
            raise TimeoutError(f"Invocation of '{path}' timed out after {timeout}s")

    def invoke_function_async(self, path: str, data: Any) -> None:
        """Fire-and-forget invocation (no response expected)."""
        msg = InvokeFunctionMessage(function_path=path, data=data)
        try:
            asyncio.get_running_loop().create_task(self._send(msg))
        except RuntimeError:
            self._enqueue(msg)
