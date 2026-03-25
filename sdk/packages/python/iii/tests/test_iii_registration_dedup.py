import asyncio
import json
import time
from types import SimpleNamespace
from typing import Any

import pytest

import iii.iii as iii_module
from iii import TriggerAction
from iii.iii import III


@pytest.fixture(autouse=True)
def reset_otel():
    yield
    # III.connect() calls init_otel() which sets global providers;
    # reset them so subsequent test files start with a clean slate.
    try:
        from iii.telemetry import shutdown_otel

        shutdown_otel()
    except Exception:
        pass
    try:
        import opentelemetry._logs._internal as _li

        _li._LOGGER_PROVIDER = None
        _li._LOGGER_PROVIDER_SET_ONCE._done = False
    except Exception:
        pass
    try:
        import opentelemetry.trace._internal as _ti

        _ti._TRACER_PROVIDER = None
        _ti._TRACER_PROVIDER_SET_ONCE._done = False
    except Exception:
        pass
    try:
        import opentelemetry.metrics._internal as _mi

        _mi._METER_PROVIDER = None
        _mi._METER_PROVIDER_SET_ONCE._done = False
    except Exception:
        pass


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


def test_preconnect_registration_sent_once(monkeypatch: pytest.MonkeyPatch) -> None:
    ws = FakeWebSocket()
    connect_calls = 0

    async def fake_connect(_addr: str, **kwargs: object) -> FakeWebSocket:
        nonlocal connect_calls
        connect_calls += 1
        return ws

    monkeypatch.setattr(iii_module.websockets, "connect", fake_connect)
    monkeypatch.setattr("iii.telemetry.init_otel", lambda **kwargs: None)
    monkeypatch.setattr("iii.telemetry.attach_event_loop", lambda loop: None)
    monkeypatch.setattr(iii_module.III, "_register_worker_metadata", lambda self: None)

    client = III("ws://fake")

    # Wait for auto-connect to complete
    time.sleep(0.05)

    async def handler(data: Any) -> Any:
        return data

    client.register_function({"id": "demo.fn"}, handler)
    client.register_trigger({"type": "cron", "function_id": "demo.fn", "config": {"cron": "* * * * * *"}})

    time.sleep(0.05)
    client.shutdown()

    reg_fn = [m for m in ws.sent if m.get("type") == "registerfunction" and m.get("id") == "demo.fn"]
    reg_trigger = [m for m in ws.sent if m.get("type") == "registertrigger" and m.get("function_id") == "demo.fn"]

    assert connect_calls == 1
    assert len(reg_fn) == 1, ws.sent
    assert len(reg_trigger) == 1, ws.sent


def test_reconnect_replays_durable_state_once_per_connection(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    sockets: list[FakeWebSocket] = []

    async def fake_connect(_addr: str, **kwargs: object) -> FakeWebSocket:
        ws = FakeWebSocket()
        sockets.append(ws)
        return ws

    monkeypatch.setattr(iii_module.websockets, "connect", fake_connect)
    monkeypatch.setattr("iii.telemetry.init_otel", lambda **kwargs: None)
    monkeypatch.setattr("iii.telemetry.attach_event_loop", lambda loop: None)
    monkeypatch.setattr(iii_module.III, "_register_worker_metadata", lambda self: None)

    client = III("ws://fake")
    time.sleep(0.05)

    async def handler(data: Any) -> Any:
        return data

    client.register_function({"id": "demo.fn"}, handler)
    client.register_trigger({"type": "cron", "function_id": "demo.fn", "config": {"cron": "* * * * * *"}})
    time.sleep(0.05)

    first_ws = client._ws
    assert first_ws is not None
    asyncio.run_coroutine_threadsafe(first_ws.close(), client._loop).result()
    client._ws = None

    client._run_on_loop(client._do_connect())
    time.sleep(0.05)
    client.shutdown()

    total_fn = sum(
        1 for ws in sockets for m in ws.sent if m.get("type") == "registerfunction" and m.get("id") == "demo.fn"
    )
    total_trigger = sum(
        1 for ws in sockets for m in ws.sent if m.get("type") == "registertrigger" and m.get("function_id") == "demo.fn"
    )

    assert total_fn == 2
    assert total_trigger == 2


def test_call_void_queued_while_disconnected_flushes_after_connect(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ws = FakeWebSocket()

    async def fake_connect(_addr: str, **kwargs: object) -> FakeWebSocket:
        return ws

    monkeypatch.setattr(iii_module.websockets, "connect", fake_connect)
    monkeypatch.setattr("iii.telemetry.init_otel", lambda **kwargs: None)
    monkeypatch.setattr("iii.telemetry.attach_event_loop", lambda loop: None)
    monkeypatch.setattr(iii_module.III, "_register_worker_metadata", lambda self: None)

    client = III("ws://fake")
    time.sleep(0.05)

    client.trigger({"function_id": "demo.fire", "payload": {"x": 1}, "action": TriggerAction.Void()})
    time.sleep(0.05)
    client.shutdown()

    invoke = [m for m in ws.sent if m.get("type") == "invokefunction" and m.get("function_id") == "demo.fire"]
    assert len(invoke) == 1
