"""Unit tests for III.add_connection_state_listener (no live engine needed)."""

import time
from types import SimpleNamespace
from typing import Any

import pytest
from iii_helpers.observability import ReconnectionConfig
from websockets.exceptions import InvalidMessage

import iii.iii as iii_module
from iii import InitOptions
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


def _patch_transport(monkeypatch: pytest.MonkeyPatch, connect) -> None:
    monkeypatch.setattr(iii_module.websockets, "connect", connect)
    monkeypatch.setattr("iii_helpers.observability.telemetry.init_otel", lambda **kwargs: None)
    monkeypatch.setattr("iii_helpers.observability.telemetry.attach_event_loop", lambda loop: None)
    monkeypatch.setattr(iii_module.III, "_register_worker_metadata", lambda self: None)


def _connected_client(monkeypatch: pytest.MonkeyPatch) -> III:
    async def fake_connect(_addr: str, **kwargs: object) -> FakeWebSocket:
        return FakeWebSocket()

    _patch_transport(monkeypatch, fake_connect)

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


def test_duplicate_subscription_unsubscribe_removes_only_its_own(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    client = _connected_client(monkeypatch)
    events: list[str] = []

    unsub1 = client.add_connection_state_listener(events.append)
    unsub2 = client.add_connection_state_listener(events.append)
    assert events == ["connected", "connected"]

    unsub1()
    unsub1()  # second call must not remove the other registration
    events.clear()
    client._set_connection_state("reconnecting")
    assert events == ["reconnecting"]

    unsub2()
    client._set_connection_state("connected")
    assert events == ["reconnecting"]

    client.shutdown()


def test_handshake_failure_keeps_retrying(monkeypatch: pytest.MonkeyPatch) -> None:
    attempts = 0

    async def flaky_connect(_addr: str, **kwargs: object) -> FakeWebSocket:
        nonlocal attempts
        attempts += 1
        if attempts <= 2:
            # Peer accepted TCP but the WS upgrade died (not an OSError):
            # must not kill the connect/reconnect task.
            raise InvalidMessage("did not receive a valid HTTP response")
        return FakeWebSocket()

    _patch_transport(monkeypatch, flaky_connect)

    client = III(
        "ws://fake",
        InitOptions(reconnection_config=ReconnectionConfig(initial_delay_ms=5, max_delay_ms=10)),
    )
    states: list[str] = []
    client.add_connection_state_listener(states.append)
    client._wait_until_connected()
    assert client.get_connection_state() == "connected"
    assert attempts == 3
    assert "connected" in states

    client.shutdown()
