"""Bridge implementation for WebSocket communication with the III Engine."""

import asyncio
import json
import logging
import os
import platform
import uuid
from dataclasses import dataclass, field
from importlib.metadata import version
from typing import Any, Awaitable, Callable

import websockets
from websockets.asyncio.client import ClientConnection

from .bridge_types import (
    FunctionInfo,
    InvocationResultMessage,
    InvokeFunctionMessage,
    MessageType,
    RegisterFunctionMessage,
    RegisterServiceMessage,
    RegisterTriggerMessage,
    RegisterTriggerTypeMessage,
    UnregisterTriggerMessage,
    UnregisterTriggerTypeMessage,
    WorkerInfo,
)
from .context import Context, with_context
from .logger import Logger
from .streams import IStream
from .triggers import Trigger, TriggerConfig, TriggerHandler
from .types import RemoteFunctionData, RemoteTriggerTypeData

RemoteFunctionHandler = Callable[[Any], Awaitable[Any]]

log = logging.getLogger("iii.bridge")


@dataclass
class BridgeOptions:
    """Options for configuring the Bridge."""

    worker_name: str | None = None


class Bridge:
    """WebSocket bridge for communication with the III Engine."""

    def __init__(self, address: str, options: BridgeOptions | None = None) -> None:
        self._address = address
        self._options = options or BridgeOptions()
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
        self._functions_available_callbacks: set[Callable[[list[FunctionInfo]], None]] = set()
        self._functions_available_trigger: Trigger | None = None
        self._functions_available_function_path: str | None = None

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

        # Register worker metadata
        self._register_worker_metadata()

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
        trigger_type_id = data.get("trigger_type")
        handler_data = self._trigger_types.get(trigger_type_id) if trigger_type_id else None

        trigger_id = data.get("id", "")
        function_path = data.get("function_path", "")
        config = data.get("config")

        result_base = {
            "type": MessageType.TRIGGER_REGISTRATION_RESULT.value,
            "id": trigger_id,
            "trigger_type": trigger_type_id,
            "function_path": function_path,
        }

        if not handler_data:
            return

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

    def register_trigger(self, trigger_type: str, function_path: str, config: Any) -> Trigger:
        trigger_id = str(uuid.uuid4())
        msg = RegisterTriggerMessage(
            id=trigger_id,
            trigger_type=trigger_type,
            function_path=function_path,
            config=config,
        )
        self._enqueue(msg)
        self._triggers[trigger_id] = msg

        def unregister() -> None:
            self._enqueue(UnregisterTriggerMessage(id=trigger_id))
            self._triggers.pop(trigger_id, None)

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

    async def list_functions(self) -> list[FunctionInfo]:
        """List all registered functions from the engine."""
        result = await self.invoke_function("engine.functions.list", {})
        functions_data = result.get("functions", [])
        return [FunctionInfo(**f) for f in functions_data]

    async def list_workers(self) -> list[WorkerInfo]:
        """List all connected workers from the engine."""
        result = await self.invoke_function("engine.workers.list", {})
        workers_data = result.get("workers", [])
        return [WorkerInfo(**w) for w in workers_data]

    def _get_worker_metadata(self) -> dict[str, Any]:
        """Get worker metadata for registration."""
        try:
            sdk_version = version("iii-sdk")
        except Exception:
            sdk_version = "unknown"

        worker_name = self._options.worker_name or f"{platform.node()}:{os.getpid()}"

        return {
            "runtime": "python",
            "version": sdk_version,
            "name": worker_name,
            "os": f"{platform.system()} {platform.release()} ({platform.machine()})",
        }

    def _register_worker_metadata(self) -> None:
        """Register this worker's metadata with the engine."""
        self.invoke_function_async("engine.workers.register", self._get_worker_metadata())

    def on_functions_available(self, callback: Callable[[list[FunctionInfo]], None]) -> Callable[[], None]:
        """Subscribe to function availability events.

        Args:
            callback: Function to call when functions become available. Receives list of FunctionInfo.

        Returns:
            Unsubscribe function that removes the callback and cleans up the trigger if no callbacks remain.
        """
        self._functions_available_callbacks.add(callback)

        if not self._functions_available_trigger:
            if not self._functions_available_function_path:
                self._functions_available_function_path = f"bridge.on_functions_available.{uuid.uuid4()}"

            function_path = self._functions_available_function_path
            if function_path not in self._functions:
                async def handler(data: dict[str, Any]) -> None:
                    functions_data = data.get("functions", [])
                    functions = [FunctionInfo(**f) for f in functions_data]
                    for cb in self._functions_available_callbacks:
                        cb(functions)

                self.register_function(function_path, handler)

            self._functions_available_trigger = self.register_trigger(
                "engine::functions-available",
                function_path,
                {}
            )

        def unsubscribe() -> None:
            self._functions_available_callbacks.discard(callback)
            if len(self._functions_available_callbacks) == 0 and self._functions_available_trigger:
                self._functions_available_trigger.unregister()
                self._functions_available_trigger = None

        return unsubscribe

    def on(self, event: str, callback: Callable[..., None]) -> Callable[[], None]:
        """Subscribe to an event.

        This is a no-op in Python due to the different event model of websockets.
        Provided for API compatibility with Node.js client.

        Args:
            event: The event name to subscribe to.
            callback: The callback function to call when the event occurs.

        Returns:
            An unsubscribe function (no-op).
        """
        def unsubscribe() -> None:
            pass

        return unsubscribe

    def create_stream(self, stream_name: str, stream: IStream[Any]) -> None:
        """Register stream functions for a given stream.

        This registers the following functions for the stream:
        - {stream_name}.get
        - {stream_name}.set
        - {stream_name}.delete
        - {stream_name}.getGroup
        - {stream_name}.listGroups
        - {stream_name}.update

        Args:
            stream_name: The name of the stream.
            stream: The stream implementation.
        """
        async def get_handler(data: Any) -> Any:
            from .streams import StreamGetInput
            input_data = StreamGetInput(**data) if isinstance(data, dict) else data
            return await stream.get(input_data)

        async def set_handler(data: Any) -> Any:
            from .streams import StreamSetInput
            input_data = StreamSetInput(**data) if isinstance(data, dict) else data
            result = await stream.set(input_data)
            return result.model_dump() if result else None

        async def delete_handler(data: Any) -> None:
            from .streams import StreamDeleteInput
            input_data = StreamDeleteInput(**data) if isinstance(data, dict) else data
            await stream.delete(input_data)

        async def get_group_handler(data: Any) -> list[Any]:
            from .streams import StreamGetGroupInput
            input_data = StreamGetGroupInput(**data) if isinstance(data, dict) else data
            return await stream.get_group(input_data)

        async def list_groups_handler(data: Any) -> list[str]:
            from .streams import StreamListGroupsInput
            input_data = StreamListGroupsInput(**data) if isinstance(data, dict) else data
            return await stream.list_groups(input_data)

        async def update_handler(data: Any) -> Any:
            from .streams import StreamUpdateInput
            input_data = StreamUpdateInput(**data) if isinstance(data, dict) else data
            result = await stream.update(input_data)
            return result.model_dump() if result else None

        self.register_function(f"{stream_name}.get", get_handler)
        self.register_function(f"{stream_name}.set", set_handler)
        self.register_function(f"{stream_name}.delete", delete_handler)
        self.register_function(f"{stream_name}.getGroup", get_group_handler)
        self.register_function(f"{stream_name}.listGroups", list_groups_handler)
        self.register_function(f"{stream_name}.update", update_handler)
