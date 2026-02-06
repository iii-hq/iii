#!/usr/bin/env python3
"""Example demonstrating the III SDK for Python with API trigger and Streams."""

import asyncio
import logging
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

    bridge.register_trigger(
        trigger_type="api",
        function_id="python.list_greetings",
        config={"api_path": "greetings", "http_method": "GET"},
    )

    bridge.register_trigger(
        trigger_type="api",
        function_id="python.get_greeting",
        config={"api_path": "greetings/:name", "http_method": "GET"},
    )

    bridge.register_trigger(
        trigger_type="api",
        function_id="python.delete_greeting",
        config={"api_path": "greetings/:name", "http_method": "DELETE"},
    )

    # Connect
    log.info(f"Connecting to {BRIDGE_URL}...")
    await bridge.connect()

    log.info("=" * 60)
    log.info("API Endpoints:")
    log.info("  GET    /greet?name=X      - Greet and save")
    log.info("  GET    /greetings         - List all greetings")
    log.info("  GET    /greetings/:name   - Get specific greeting")
    log.info("  DELETE /greetings/:name   - Delete greeting")
    log.info("=" * 60)

    # Keep running
    log.info("Listening for requests (press Ctrl+C to exit)...")
    try:
        while True:
            await asyncio.sleep(1)
    except KeyboardInterrupt:
        log.info("Shutting down...")

    await bridge.disconnect()


if __name__ == "__main__":
    asyncio.run(main())
