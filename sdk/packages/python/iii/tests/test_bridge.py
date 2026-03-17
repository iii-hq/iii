"""Integration tests for bridge operations."""

import asyncio

import pytest

from iii import TriggerAction
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
    functions = iii_client.list_functions()
    assert isinstance(functions, list)


@pytest.mark.asyncio
async def test_register_and_invoke_function(iii_client: III):
    """Registering a function and invoking it returns the expected result."""
    received = []

    async def echo_handler(data):
        received.append(data)
        return {"echoed": data}

    fn = iii_client.register_function({"id": "test.bridge.py.echo"}, echo_handler)
    await asyncio.sleep(0.3)

    try:
        result = iii_client.trigger(
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
async def test_invoke_function_fire_and_forget(iii_client: III):
    """Void trigger fires without waiting for the result."""
    received = []
    received_event = asyncio.Event()

    async def receiver_handler(data):
        received.append(data)
        received_event.set()
        return {}

    fn = iii_client.register_function({"id": "test.bridge.py.receiver"}, receiver_handler)
    await asyncio.sleep(0.3)

    try:
        result = iii_client.trigger({
            "function_id": "test.bridge.py.receiver",
            "payload": {"value": 42},
            "action": TriggerAction.Void(),
        })

        assert result is None

        await asyncio.wait_for(received_event.wait(), timeout=5.0)
        assert received[0]["value"] == 42
    finally:
        fn.unregister()


@pytest.mark.asyncio
async def test_list_registered_functions(iii_client: III):
    """Registered function IDs appear in list_functions()."""
    fn1 = iii_client.register_function({"id": "test.bridge.py.list.func1"}, lambda _: {})
    fn2 = iii_client.register_function({"id": "test.bridge.py.list.func2"}, lambda _: {})
    await asyncio.sleep(0.3)

    try:
        functions = iii_client.list_functions()
        function_ids = [f.function_id for f in functions]

        assert "test.bridge.py.list.func1" in function_ids
        assert "test.bridge.py.list.func2" in function_ids
    finally:
        fn1.unregister()
        fn2.unregister()


@pytest.mark.asyncio
async def test_reject_non_existent_function(iii_client: III):
    """Triggering a non-existent function raises an error."""
    with pytest.raises(Exception):
        iii_client.trigger({
            "function_id": "nonexistent.function.py",
            "payload": {},
            "timeout_ms": 2000,
        })
