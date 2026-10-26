import asyncio

import pytest
from iii_helpers.observability import OtelConfig, ReconnectionConfig, current_span_id, current_trace_id
from iii_helpers.observability.telemetry_exporters import SharedEngineConnection


def test_otel_config_defaults_disabled():
    cfg = OtelConfig()
    assert cfg.enabled is None  # enabled=None means "read from env"


def test_reconnection_config_has_initial_delay():
    cfg = ReconnectionConfig()
    assert cfg.initial_delay_ms > 0


def test_current_ids_none_outside_span():
    assert current_span_id() is None
    assert current_trace_id() is None


async def test_telemetry_handshake_carries_the_headers_on_connect_and_reconnect(monkeypatch):
    headers = {"Authorization": "Bearer secret"}
    calls: list[dict[str, object]] = []
    reconnected = asyncio.Event()
    real_sleep = asyncio.sleep

    def refuse(_url, **kwargs):
        calls.append(kwargs)
        if len(calls) == 2:
            reconnected.set()
        raise ConnectionError("engine down")

    async def no_backoff(_delay):
        await real_sleep(0)

    monkeypatch.setattr("websockets.connect", refuse)
    monkeypatch.setattr("iii_helpers.observability.telemetry_exporters.asyncio.sleep", no_backoff)

    connection = SharedEngineConnection("ws://engine.test/otel", headers)
    connection.start(asyncio.get_running_loop())
    await asyncio.wait_for(reconnected.wait(), timeout=5)
    await connection.shutdown()

    assert [call["additional_headers"] for call in calls[:2]] == [headers, headers]
