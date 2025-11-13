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


async def invoke_echo(bridge_url: str, text: str, timeout: float) -> dict | str | None:
    invocation_id = str(uuid.uuid4())
    payload = {
        "type": "invokefunction",
        "invocationId": invocation_id,
        "functionPath": "engine.echo",
        "data": {"text": text, "from": "python-example"},
    }

    async with websockets.connect(bridge_url) as ws:
        print(f"Connected to {bridge_url}, sending invocation {invocation_id}")
        await ws.send(json.dumps(payload))

        async def wait_for_result():
            async for raw in ws:
                message = json.loads(raw)
                msg_type = message.get("type")

                if msg_type == "ping":
                    await ws.send(json.dumps({"type": "pong"}))
                    continue

                if msg_type == "invocationresult" and message.get("invocationId") == invocation_id:
                    if message.get("error"):
                        raise RuntimeError(f"Invocation failed: {message['error']}")
                    return message.get("result")

                # Ignore other traffic (e.g., other invocation results)
                print(f"Ignoring message: {message}")

            raise RuntimeError("WebSocket closed before receiving result")

        return await asyncio.wait_for(wait_for_result(), timeout=timeout)


def main() -> None:
    args = parse_args()
    try:
        result = asyncio.run(invoke_echo(args.bridge_url, args.text, args.timeout))
    except Exception as exc:  # noqa: BLE001 - surface helpful message to caller
        print(f"Invocation failed: {exc}", file=sys.stderr)
        sys.exit(1)

    print("engine.echo replied with:\n" + json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
