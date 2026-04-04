# motia/tests/conftest.py
"""Pytest configuration and fixtures for integration tests.

These tests require a running III engine instance. Start the engine manually:

    iii --config tests/fixtures/config-test.yaml

Or set III_ENGINE_PATH and use the run_integration_tests.sh script.
"""

import json
import time
from typing import Generator

import pytest
from iii.iii import III

TEST_ENGINE_PORT = 49199
TEST_API_PORT = 3199
TEST_ENGINE_URL = f"ws://localhost:{TEST_ENGINE_PORT}"
TEST_API_URL = f"http://localhost:{TEST_API_PORT}"


def flush_bridge_queue(bridge) -> None:
    """Flush the bridge queue."""
    while bridge._queue and bridge._ws:
        bridge._schedule_on_loop(bridge._ws.send(json.dumps(bridge._queue.pop(0))))
    time.sleep(0.05)


def wait_for_registration(bridge, function_id: str, timeout: float = 5.0) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            result = bridge.trigger({"function_id": "engine::functions::list", "payload": {}})
            functions = result.get("functions", []) if isinstance(result, dict) else []
            ids = [f.get("function_id") for f in functions if isinstance(f, dict)]
            if function_id in ids:
                return
        except Exception:
            pass
        time.sleep(0.1)
    raise TimeoutError(f"Function {function_id} was not registered within {timeout}s")


@pytest.fixture
def bridge() -> Generator:
    bridge_instance = III(TEST_ENGINE_URL)
    bridge_instance._wait_until_connected()
    time.sleep(0.1)
    yield bridge_instance
    bridge_instance.shutdown()


@pytest.fixture
def api_url() -> str:
    return TEST_API_URL


@pytest.fixture
def engine_url() -> str:
    return TEST_ENGINE_URL
