#!/usr/bin/env python3
"""Minimal websocket client that invokes `engine.echo` and prints the reply."""
from __future__ import annotations

import argparse
import asyncio
import json
import os
import sys
import uuid

import websockets

DEFAULT_BRIDGE_URL = os.environ.get("III_BRIDGE_URL", "ws://127.0.0.1:49134")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Invoke engine.echo via the bridge")
    parser.add_argument(
        "--bridge-url",
        default=DEFAULT_BRIDGE_URL,
        help="Bridge websocket URL (default: %(default)s)",
    )
    parser.add_argument(
        "--text",
        default="Hello from Python!",
        help="Text payload to echo",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=5.0,
        help="Seconds to wait for the echo result",
    )
    return parser.parse_args()


async def invoke_echo(ws, text: str, timeout) -> dict | str | None:
    invocation_id = str(uuid.uuid4())
    payload = {
        "type": "invokefunction",
        "invocationId": invocation_id,
        "functionPath": "engine.echo",
        "data": {"text": text, "from": "python-example"},
    }

    await ws.send(json.dumps(payload))

    async def wait_for_result():
        async for raw in ws:
            message = json.loads(raw)
            msg_type = message.get("type")

            if msg_type == "ping":
                await ws.send(json.dumps({"type": "pong"}))
                continue

            if msg_type == "invocationresult" and message.get("invocationId") == invocation_id:
                print(f"Received invocation result message: {message}")
                if message.get("error"):
                    raise RuntimeError(f"Invocation failed: {message['error']}")
                return message.get("result")

            # Ignore other traffic (e.g., other invocation results)
            print(f"Ignoring message: {message}")

        raise RuntimeError("WebSocket closed before receiving result")

    return await asyncio.wait_for(wait_for_result(), timeout=timeout)


async def registerservice(ws, timeout) -> dict | str | None:
    function_id = "46ce43ec-f681-4b65-a078-1fc7ad9b06f8"
    payload = {
        "type": "registerservice",
        "id": function_id,
        "name": "engine.echo",
        "description": "A simple echo function",
    }

    await ws.send(json.dumps(payload))

    async def wait_for_result():
        async for raw in ws:
            message = json.loads(raw)
            msg_type = message.get("type")

            if msg_type == "ping":
                await ws.send(json.dumps({"type": "pong"}))
                continue

            if msg_type == "registerservice" and message.get("id") == function_id:
                print(f"Received register service result message: {message}")
                return message.get("result")

            # Ignore other traffic (e.g., other invocation results)
            print(f"Ignoring message: {message}")

        raise RuntimeError("WebSocket closed before receiving result")

    return await asyncio.wait_for(wait_for_result(), timeout=timeout)


async def invoke_methods(bridge_url: str, text: str, timeout: float):

    async with websockets.connect(bridge_url) as ws:
        # await invoke_echo(ws, text, timeout)
        await registerservice(ws, timeout)

    raise RuntimeError("WebSocket closed before receiving result")


def main() -> None:
    args = parse_args()
    try:
        result = asyncio.run(invoke_methods(args.bridge_url, args.text, args.timeout))
    except Exception as exc:  # noqa: BLE001 - surface helpful message to caller
        print(f"Invocation failed: {exc}", file=sys.stderr)
        sys.exit(1)

    print("engine.echo replied with:\n" + json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
