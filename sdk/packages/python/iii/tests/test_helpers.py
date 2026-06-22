"""Tests for the public helpers submodule (mirrors Rust/Node helpers parity)."""

from __future__ import annotations

import inspect
import json
from types import SimpleNamespace
from typing import Any

import pytest

import iii.iii as iii_module
from iii import InitOptions
from iii.iii import III


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


def _patch_ws(monkeypatch: pytest.MonkeyPatch) -> FakeWebSocket:
    ws = FakeWebSocket()

    async def fake_connect(_: str, **kwargs: object) -> FakeWebSocket:
        return ws

    monkeypatch.setattr(iii_module.websockets, "connect", fake_connect)
    monkeypatch.setattr("iii_helpers.observability.telemetry.init_otel", lambda **kwargs: None)
    monkeypatch.setattr("iii_helpers.observability.telemetry.attach_event_loop", lambda loop: None)
    monkeypatch.setattr(iii_module.III, "_register_worker_metadata", lambda self: None)
    return ws


def test_helpers_module_exports_expected_names() -> None:
    from iii import helpers

    expected = {
        "ChannelDirection",
        "ChannelItem",
        "create_channel",
        "create_channel_async",
        "create_stream",
        "extract_channel_refs",
        "is_channel_ref",
    }
    actual = set(helpers.__all__)
    missing = expected - actual
    assert not missing, f"missing from helpers.__all__: {missing}"
    for name in expected:
        assert hasattr(helpers, name), f"helpers module missing attribute: {name}"


def test_helpers_free_functions_take_iii_first() -> None:
    from iii import helpers

    for name in (
        "create_channel",
        "create_channel_async",
        "create_stream",
    ):
        sig = inspect.signature(getattr(helpers, name))
        params = list(sig.parameters)
        assert params and params[0] == "iii", f"{name} signature: {sig}"


def test_is_channel_ref_works_via_helpers() -> None:
    from iii import helpers

    assert helpers.is_channel_ref({}) is False
    assert helpers.is_channel_ref(
        {"channel_id": "c", "access_key": "k", "direction": "read"}
    ) is True
    assert helpers.is_channel_ref(
        {"channel_id": "c", "access_key": "k", "direction": "garbage"}
    ) is False


def test_extract_channel_refs_walks_nested_structures() -> None:
    from iii import helpers

    assert helpers.extract_channel_refs({}) == []

    ref = {"channel_id": "c1", "access_key": "k1", "direction": "read"}
    refs = helpers.extract_channel_refs({"input": ref})
    assert len(refs) == 1
    path, channel_ref = refs[0]
    assert path == "input"
    assert channel_ref.channel_id == "c1"

    nested = helpers.extract_channel_refs(
        {"items": [{"writer": ref}, {"writer": ref}]}
    )
    assert {p for p, _ in nested} == {"items[0].writer", "items[1].writer"}


def test_channel_direction_string_values() -> None:
    from iii.helpers import ChannelDirection

    assert ChannelDirection.READ.value == "read"
    assert ChannelDirection.WRITE.value == "write"


def test_channel_item_constructors() -> None:
    from iii.helpers import ChannelItem

    text = ChannelItem.text_item("hi")
    assert text.is_text and not text.is_binary
    assert text.text == "hi"

    binary = ChannelItem.binary_item(b"\x00\x01")
    assert binary.is_binary and not binary.is_text
    assert binary.binary == b"\x00\x01"


def test_iii_no_longer_exposes_relocated_methods(monkeypatch: pytest.MonkeyPatch) -> None:
    _patch_ws(monkeypatch)
    client = III("ws://fake", InitOptions())

    try:
        for name in (
            "create_channel",
            "create_channel_async",
            "create_stream",
        ):
            assert not hasattr(client, name), f"client still has {name}"
    finally:
        client.shutdown()


def test_iii_client_protocol_no_longer_declares_relocated_methods() -> None:
    """The :class:`iii.IIIClient` Protocol must not declare the relocated methods."""
    from iii import IIIClient

    for name in (
        "create_channel",
        "create_stream",
    ):
        assert name not in IIIClient.__dict__, (
            f"IIIClient Protocol still declares {name}"
        )


def test_init_no_longer_exports_relocated_channel_items() -> None:
    import iii

    for name in (
        "extract_channel_refs",
        "is_channel_ref",
        "ChannelDirection",
        "ChannelItem",
    ):
        assert name not in iii.__all__, f"{name} still in iii.__all__"


def test_iii_register_and_unregister_trigger_type_round_trip(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    from iii.protocol import RegisterTriggerTypeInput
    from iii.triggers import TriggerConfig, TriggerHandler

    class DummyHandler(TriggerHandler[Any]):
        async def register_trigger(self, config: TriggerConfig[Any]) -> None:
            return None

        async def unregister_trigger(self, config: TriggerConfig[Any]) -> None:
            return None

    _patch_ws(monkeypatch)
    client = III("ws://fake", InitOptions())
    try:
        trigger_type = RegisterTriggerTypeInput(
            id="helpers.test", description="from helpers"
        )

        ref = client.register_trigger_type(trigger_type, DummyHandler())
        assert "helpers.test" in client._trigger_types
        assert ref is not None

        client.unregister_trigger_type(trigger_type)
        assert "helpers.test" not in client._trigger_types
    finally:
        client.shutdown()
