#!/usr/bin/env python3
"""Async Python client for the engine server.

The client mirrors the Rust client's behaviour: it registers a `math.add` method,
handles incoming `call` messages, responds with results, and emits heartbeat
`notify` frames on a configurable interval. Communication uses the same length-
delimited JSON framing as Tokio's `LengthDelimitedCodec` (4-byte big-endian
length prefix).
"""
from __future__ import annotations

import argparse
import asyncio
import json
import os
import signal
import struct
import sys
import uuid
from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Any, Awaitable, Callable, Dict, Optional

LENGTH_PREFIX = struct.Struct(">I")
MAX_FRAME_LENGTH = 64 * 1024 * 1024  # 64 MiB, must match server config


@dataclass
class MethodSpec:
    name: str
    params_schema: Dict[str, Any]
    result_schema: Dict[str, Any]
    handler: Callable[[Dict[str, Any]], Awaitable[Any] | Any]


class EngineClient:
    def __init__(
        self,
        addr: str,
        heartbeat_interval: float,
        heartbeat_method: str = "client.heartbeat",
        runtime_limit: Optional[float] = None,
    ) -> None:
        host, port_str = addr.split(":", 1)
        self.host = host
        self.port = int(port_str)
        self.heartbeat_interval = heartbeat_interval
        self.heartbeat_method = heartbeat_method
        self.runtime_limit = runtime_limit
        self.client_id = uuid.uuid4()
        self.reader: Optional[asyncio.StreamReader] = None
        self.writer: Optional[asyncio.StreamWriter] = None
        self.writer_lock = asyncio.Lock()
        self.methods: Dict[str, MethodSpec] = {}
        self.listen_task: Optional[asyncio.Task[None]] = None
        self.heartbeat_task: Optional[asyncio.Task[None]] = None
        self.stop_event = asyncio.Event()

    def register_method(self, spec: MethodSpec) -> None:
        self.methods[spec.name] = spec

    async def run(self) -> None:
        self.reader, self.writer = await asyncio.open_connection(self.host, self.port)
        await self._send_register()
        self.listen_task = asyncio.create_task(self._listen_loop(), name="engine-listen")
        if self.heartbeat_interval > 0:
            self.heartbeat_task = asyncio.create_task(
                self._heartbeat_loop(), name="engine-heartbeat"
            )
        try:
            if self.runtime_limit is not None:
                await asyncio.wait_for(self.stop_event.wait(), timeout=self.runtime_limit)
            else:
                await self.stop_event.wait()
        except asyncio.TimeoutError:
            print(f"runtime limit {self.runtime_limit}s reached, shutting down")
        finally:
            await self.shutdown()

    async def shutdown(self) -> None:
        if self.heartbeat_task:
            self.heartbeat_task.cancel()
        if self.listen_task:
            self.listen_task.cancel()
        if self.writer:
            try:
                self.writer.close()
                await self.writer.wait_closed()
            except Exception:
                pass

    async def _send_register(self) -> None:
        methods_payload = [
            {
                "name": spec.name,
                "params_schema": spec.params_schema,
                "result_schema": spec.result_schema,
            }
            for spec in self.methods.values()
        ]
        payload = {
            "type": "register",
            "from": str(self.client_id),
            "methods": methods_payload,
        }
        await self._send_json(payload)

    async def _heartbeat_loop(self) -> None:
        try:
            while True:
                notify = {
                    "type": "notify",
                    "from": str(self.client_id),
                    "to": None,
                    "method": self.heartbeat_method,
                    "params": {
                        "ts": datetime.now(timezone.utc).isoformat(),
                    },
                }
                await self._send_json(notify)
                await asyncio.sleep(self.heartbeat_interval)
        except asyncio.CancelledError:
            pass

    async def _listen_loop(self) -> None:
        assert self.reader is not None
        try:
            while True:
                msg = await self._read_json()
                if msg is None:
                    print("server closed connection", file=sys.stderr)
                    break
                await self._dispatch(msg)
        except asyncio.CancelledError:
            pass
        except Exception as exc:
            print(f"listener error: {exc}", file=sys.stderr)
        finally:
            self.stop_event.set()

    async def _dispatch(self, msg: Dict[str, Any]) -> None:
        mtype = msg.get("type")
        if mtype == "call":
            await self._handle_call(msg)
        elif mtype == "notify":
            method = msg.get("method")
            print(f"notify<{method}>: {json.dumps(msg.get('params'))}")
        elif mtype == "ping":
            pong = {"type": "pong", "from": str(self.client_id)}
            await self._send_json(pong)
        else:
            print(f"received {mtype}: {json.dumps(msg)}")

    async def _handle_call(self, msg: Dict[str, Any]) -> None:
        method = msg.get("method")
        spec = self.methods.get(method)
        call_id = msg.get("id")
        if spec is None:
            error_payload = {
                "type": "error",
                "id": call_id,
                "from": str(self.client_id),
                "code": "method_not_found",
                "message": f"no handler for {method}",
            }
            await self._send_json(error_payload)
            return

        try:
            params = msg.get("params", {})
            result = spec.handler(params)
            if asyncio.iscoroutine(result):
                result = await result
            reply = {
                "type": "result",
                "id": call_id,
                "from": str(self.client_id),
                "ok": True,
                "result": result,
            }
            await self._send_json(reply)
        except Exception as exc:  # noqa: BLE001 - send structured error
            err_reply = {
                "type": "result",
                "id": call_id,
                "from": str(self.client_id),
                "ok": False,
                "error": {
                    "code": "client_error",
                    "message": str(exc),
                },
            }
            await self._send_json(err_reply)

    async def _send_json(self, payload: Dict[str, Any]) -> None:
        data = json.dumps(payload, separators=(",", ":")).encode("utf-8")
        if len(data) > MAX_FRAME_LENGTH:
            raise ValueError("frame too large")
        if self.writer is None:
            raise RuntimeError("writer not ready")
        async with self.writer_lock:
            self.writer.write(LENGTH_PREFIX.pack(len(data)))
            self.writer.write(data)
            await self.writer.drain()

    async def _read_json(self) -> Optional[Dict[str, Any]]:
        assert self.reader is not None
        try:
            header = await self.reader.readexactly(LENGTH_PREFIX.size)
        except asyncio.IncompleteReadError:
            return None
        (size,) = LENGTH_PREFIX.unpack(header)
        if size > MAX_FRAME_LENGTH:
            raise ValueError("incoming frame exceeds limit")
        data = await self.reader.readexactly(size)
        return json.loads(data.decode("utf-8"))


async def math_add_handler(params: Dict[str, Any]) -> float:
    try:
        a = float(params["a"])
        b = float(params["b"])
    except (KeyError, TypeError, ValueError) as exc:
        raise ValueError("params must contain numeric 'a' and 'b'") from exc
    return a + b


def build_client(addr: str, heartbeat: float, run_for: Optional[float]) -> EngineClient:
    client = EngineClient(addr=addr, heartbeat_interval=heartbeat, runtime_limit=run_for)
    client.register_method(
        MethodSpec(
            name="math.add",
            params_schema={
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": {
                    "a": {"type": "number"},
                    "b": {"type": "number"},
                },
                "required": ["a", "b"],
                "additionalProperties": False,
            },
            result_schema={"type": "number"},
            handler=math_add_handler,
        )
    )
    return client


async def async_main(args: argparse.Namespace) -> None:
    client = build_client(addr=args.addr, heartbeat=args.heartbeat, run_for=args.run_for)
    await client.run()


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Python client for the engine server")
    run_for_env = os.environ.get("ENGINE_RUN_FOR")
    run_for_default = float(run_for_env) if run_for_env is not None else None
    parser.add_argument(
        "--addr",
        default=os.environ.get("ENGINE_ADDR", "127.0.0.1:8080"),
        help="host:port of the engine server (default: %(default)s)",
    )
    parser.add_argument(
        "--heartbeat",
        type=float,
        default=float(os.environ.get("ENGINE_HEARTBEAT", 5.0)),
        help="seconds between heartbeat notifications (0 disables)",
    )
    parser.add_argument(
        "--run-for",
        type=float,
        default=run_for_default,
        help="seconds to run before shutting down (default: run until server closes)",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    loop = asyncio.new_event_loop()
    asyncio.set_event_loop(loop)

    for sig in (signal.SIGINT, signal.SIGTERM):
        try:
            loop.add_signal_handler(sig, loop.stop)
        except NotImplementedError:
            # Windows doesn't allow custom signal handlers in Proactor loop.
            pass

    try:
        loop.run_until_complete(async_main(args))
    finally:
        loop.run_until_complete(loop.shutdown_asyncgens())
        loop.close()


if __name__ == "__main__":
    main()
