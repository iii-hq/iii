"""Tests for the synchronous public API."""

import json
import time
from types import SimpleNamespace
from typing import Any

import pytest

import iii.iii as iii_module
from iii import InitOptions, RegisterServiceInput, RegisterTriggerTypeInput
from iii.iii import III
from iii.triggers import TriggerConfig, TriggerHandler


class FakeWebSocket:
    def __init__(self) -> None:
        self.sent: list[dict[str, Any]] = []
        self.state = SimpleNamespace(name="OPEN")

    async def send(self, payload: str) -> None:
        self.sent.append(json.loads(payload))

    async def close(self) -> None:
        self.state = SimpleNamespace(name="CLOSED")

    def __aiter__(self) -> "FakeWebSocket":
        return self

    async def __anext__(self) -> Any:
        raise StopAsyncIteration


class DummyTriggerHandler(TriggerHandler[Any]):
    async def register_trigger(self, config: TriggerConfig[Any]) -> None:
        return None

    async def unregister_trigger(self, config: TriggerConfig[Any]) -> None:
        return None


def _patch_ws(monkeypatch: pytest.MonkeyPatch) -> FakeWebSocket:
    ws = FakeWebSocket()

    async def fake_connect(_: str) -> FakeWebSocket:
        return ws

    monkeypatch.setattr(iii_module.websockets, "connect", fake_connect)
    monkeypatch.setattr("iii.telemetry.init_otel", lambda **kwargs: None)
    monkeypatch.setattr("iii.telemetry.attach_event_loop", lambda loop: None)
    monkeypatch.setattr(iii_module.III, "_register_worker_metadata", lambda self: None)
    return ws


def test_iii_creates_background_thread(monkeypatch: pytest.MonkeyPatch) -> None:
    """III.__init__ should start a non-daemon background thread with an event loop."""
    _patch_ws(monkeypatch)
    client = III("ws://fake", InitOptions())
    assert hasattr(client, "_loop")
    assert hasattr(client, "_thread")
    assert client._thread.is_alive()
    assert client._thread.daemon is False
    client.shutdown()


def test_register_worker_connects(monkeypatch: pytest.MonkeyPatch) -> None:
    """register_worker() should return an already-connected client."""
    _patch_ws(monkeypatch)
    client = III("ws://fake", InitOptions())
    time.sleep(0.05)

    client._wait_until_connected()
    assert client.get_connection_state() == "connected"

    client.shutdown()


def test_shutdown_is_sync(monkeypatch: pytest.MonkeyPatch) -> None:
    """shutdown() should be a synchronous call."""
    _patch_ws(monkeypatch)
    client = III("ws://fake", InitOptions())
    time.sleep(0.05)

    result = client.shutdown()
    assert result is None
    assert client.get_connection_state() == "disconnected"


def test_background_thread_stops_on_shutdown(monkeypatch: pytest.MonkeyPatch) -> None:
    """After shutdown(), the background thread should stop."""
    _patch_ws(monkeypatch)
    client = III("ws://fake", InitOptions())
    time.sleep(0.05)

    thread = client._thread
    client.shutdown()

    assert not thread.is_alive()


def test_trigger_is_sync(monkeypatch: pytest.MonkeyPatch) -> None:
    """trigger() should be synchronous and return the result directly."""
    _patch_ws(monkeypatch)
    client = III("ws://fake", InitOptions())
    time.sleep(0.05)

    def echo_handler(data: Any) -> Any:
        return {"echo": data}

    client.register_function({"id": "test.echo"}, echo_handler)
    time.sleep(0.05)

    from iii import TriggerAction

    result = client.trigger({
        "function_id": "test.echo",
        "payload": {"hello": "world"},
        "action": TriggerAction.Void(),
    })
    assert result is None

    client.shutdown()


def test_register_function_accepts_sync_handler(monkeypatch: pytest.MonkeyPatch) -> None:
    """register_function should accept plain sync functions."""
    ws = _patch_ws(monkeypatch)
    client = III("ws://fake", InitOptions())
    time.sleep(0.05)

    def greet(data: Any) -> Any:
        return {"message": f"Hello, {data['name']}!"}

    ref = client.register_function({"id": "test.greet"}, greet)
    assert ref.id == "test.greet"
    time.sleep(0.05)

    reg_msgs = [m for m in ws.sent if m.get("type") == "registerfunction" and m.get("id") == "test.greet"]
    assert len(reg_msgs) == 1

    client.shutdown()


def test_register_function_accepts_async_handler(monkeypatch: pytest.MonkeyPatch) -> None:
    """register_function should still accept async functions."""
    _patch_ws(monkeypatch)
    client = III("ws://fake", InitOptions())
    time.sleep(0.05)

    async def greet(data: Any) -> Any:
        return {"message": f"Hello, {data['name']}!"}

    ref = client.register_function({"id": "test.greet.async"}, greet)
    assert ref.id == "test.greet.async"

    client.shutdown()


def test_register_service_accepts_input_object(monkeypatch: pytest.MonkeyPatch) -> None:
    """register_service should store services by the provided service id."""
    _patch_ws(monkeypatch)
    client = III("ws://fake", InitOptions())
    time.sleep(0.05)

    client.register_service(
        RegisterServiceInput(
            id="svc.test",
            name="Test Service",
            description="service description",
            parent_service_id="svc.parent",
        )
    )

    assert "svc.test" in client._services
    service = client._services["svc.test"]
    assert service.name == "Test Service"
    assert service.parent_service_id == "svc.parent"

    client.shutdown()


def test_register_and_unregister_trigger_type_accept_input_object(monkeypatch: pytest.MonkeyPatch) -> None:
    """Trigger type registration should accept input objects symmetrically."""
    _patch_ws(monkeypatch)
    client = III("ws://fake", InitOptions())
    time.sleep(0.05)
    trigger_type = RegisterTriggerTypeInput(id="trigger.test", description="Trigger description")

    client.register_trigger_type(trigger_type, DummyTriggerHandler())

    assert "trigger.test" in client._trigger_types
    assert client._trigger_types["trigger.test"].message.description == "Trigger description"

    client.unregister_trigger_type(trigger_type)

    assert "trigger.test" not in client._trigger_types

    client.shutdown()


def test_public_methods_are_sync(monkeypatch: pytest.MonkeyPatch) -> None:
    """list_functions, list_triggers, create_channel should be sync."""
    _patch_ws(monkeypatch)
    client = III("ws://fake", InitOptions())
    time.sleep(0.05)

    import inspect
    assert not inspect.iscoroutinefunction(client.list_functions)
    assert not inspect.iscoroutinefunction(client.list_triggers)
    assert not inspect.iscoroutinefunction(client.create_channel)

    client.shutdown()


def test_register_worker_is_sync() -> None:
    """register_worker() should be a synchronous function."""
    import inspect

    from iii import register_worker

    assert not inspect.iscoroutinefunction(register_worker)
