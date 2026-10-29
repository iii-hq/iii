"""Integration tests for bridge operations."""

import asyncio

import pytest

from iii import CallAction
from iii.iii import III


async def wait_for(condition, timeout=5.0, interval=0.1):
    """Poll until condition() is truthy or timeout."""
    deadline = asyncio.get_event_loop().time() + timeout
    while asyncio.get_event_loop().time() < deadline:
        if condition():
            return
        await asyncio.sleep(interval)
    raise TimeoutError(f"Condition not met within {timeout}s")


@pytest.mark.asyncio
async def test_connect_successfully(iii_client: III):
    """SDK connects to the engine and can list functions."""
    result = iii_client.call({"function_id": "engine::functions::list", "payload": {}})
    functions = result.get("functions", [])
    assert isinstance(functions, list)


@pytest.mark.asyncio
async def test_register_and_invoke_function(iii_client: III):
    """Registering a function and invoking it returns the expected result."""
    received = []

    async def echo_handler(data):
        received.append(data)
        return {"echoed": data}

    fn = iii_client.register_function("test.bridge.py.echo", echo_handler)
    await asyncio.sleep(0.3)

    try:
        result = iii_client.call(
            {
                "function_id": "test.bridge.py.echo",
                "payload": {"message": "hello"},
            }
        )

        assert result is not None
        assert result["echoed"]["message"] == "hello"
        assert received[0]["message"] == "hello"
    finally:
        fn.unregister()


@pytest.mark.asyncio
async def test_non_ascii_payload_round_trips_through_the_engine_at_its_utf8_size(iii_client: III):
    """30 MiB of emoji is a ~30 MiB envelope, as from Node or Rust (with \\u escapes it was ~90 MiB, over the limit)."""

    def echo(data):
        return data

    # Through the engine, not dispatched in-process.
    fn = iii_client.register_function("test.bridge.py.echo-utf8", echo, local_dispatch=False)
    await asyncio.sleep(0.3)

    try:
        text = "😀" * (30 * 1024 * 1024 // 4)
        result = iii_client.call({"function_id": "test.bridge.py.echo-utf8", "payload": text, "timeout_ms": 60_000})
        assert result == text
    finally:
        fn.unregister()


@pytest.mark.asyncio
async def test_invoke_function_fire_and_forget(iii_client: III):
    """A void call fires without waiting for the result."""
    received = []
    received_event = asyncio.Event()

    async def receiver_handler(data):
        received.append(data)
        received_event.set()
        return {}

    fn = iii_client.register_function("test.bridge.py.receiver", receiver_handler)
    await asyncio.sleep(0.3)

    try:
        result = iii_client.call(
            {
                "function_id": "test.bridge.py.receiver",
                "payload": {"value": 42},
                "action": CallAction.Void(),
            }
        )

        assert result is None

        await asyncio.wait_for(received_event.wait(), timeout=5.0)
        assert received[0]["value"] == 42
    finally:
        fn.unregister()


@pytest.mark.asyncio
async def test_list_registered_functions(iii_client: III):
    """Registered function IDs appear in the engine functions list."""
    fn1 = iii_client.register_function("test.bridge.py.list.func1", lambda _: {})
    fn2 = iii_client.register_function("test.bridge.py.list.func2", lambda _: {})
    await asyncio.sleep(0.3)

    try:
        result = iii_client.call({"function_id": "engine::functions::list", "payload": {}})
        functions = result.get("functions", [])
        function_ids = [f["function_id"] for f in functions]

        assert "test.bridge.py.list.func1" in function_ids
        assert "test.bridge.py.list.func2" in function_ids
    finally:
        fn1.unregister()
        fn2.unregister()


@pytest.mark.asyncio
async def test_reject_non_existent_function(iii_client: III):
    """Calling a non-existent function raises an error."""
    with pytest.raises(Exception):
        iii_client.call(
            {
                "function_id": "nonexistent.function.py",
                "payload": {},
                "timeout_ms": 2000,
            }
        )
