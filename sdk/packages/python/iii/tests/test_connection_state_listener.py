"""Unit tests for III.add_connection_state_listener (no live engine needed)."""

import time
from types import SimpleNamespace
from typing import Any

import pytest

import iii.iii as iii_module
from iii.iii import III


class FakeWebSocket:
    def __init__(self) -> None:
        self.state = SimpleNamespace(name="OPEN")

    async def send(self, payload: str) -> None:
        pass

    async def close(self) -> None:
        self.state = SimpleNamespace(name="CLOSED")

    def __aiter__(self) -> "FakeWebSocket":
        return self

    async def __anext__(self) -> Any:
        raise StopAsyncIteration


def _connected_client(monkeypatch: pytest.MonkeyPatch) -> III:
    async def fake_connect(_addr: str, **kwargs: object) -> FakeWebSocket:
        return FakeWebSocket()

    monkeypatch.setattr(iii_module.websockets, "connect", fake_connect)
    monkeypatch.setattr("iii_helpers.observability.telemetry.init_otel", lambda **kwargs: None)
    monkeypatch.setattr("iii_helpers.observability.telemetry.attach_event_loop", lambda loop: None)
    monkeypatch.setattr(iii_module.III, "_register_worker_metadata", lambda self: None)

    client = III("ws://fake")
    client._wait_until_connected()
    time.sleep(0.05)
    assert client.get_connection_state() == "connected"
    return client


def test_immediate_fire_transitions_dedup_and_shutdown(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    client = _connected_client(monkeypatch)
    events: list[str] = []

    client.add_connection_state_listener(events.append)
    assert events == ["connected"]  # fired immediately with current state

    client._set_connection_state("reconnecting")
    client._set_connection_state("connected")
    client._set_connection_state("connected")  # same state -> no fire
    assert events == ["connected", "reconnecting", "connected"]

    client.shutdown()
    assert events == ["connected", "reconnecting", "connected", "disconnected"]


def test_raising_listener_isolated_and_unsubscribe_idempotent(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    client = _connected_client(monkeypatch)
    events: list[str] = []

    def bad(state: str) -> None:
        raise RuntimeError("boom")

    client.add_connection_state_listener(bad)  # immediate fire raises, is swallowed
    unsubscribe = client.add_connection_state_listener(events.append)
    assert events == ["connected"]

    client._set_connection_state("reconnecting")
    assert events == ["connected", "reconnecting"]  # bad didn't block delivery

    unsubscribe()
    unsubscribe()  # idempotent
    client._set_connection_state("connected")
    assert events == ["connected", "reconnecting"]

    client.shutdown()  # fires "disconnected" into bad -> swallowed, no crash
    assert events == ["connected", "reconnecting"]
