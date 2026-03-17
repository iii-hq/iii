"""Tests for the two-arg register_function() pattern with RegisterFunctionInput."""

import json
import time
from types import SimpleNamespace
from typing import Any

import pytest

import iii.iii as iii_module
from iii import InitOptions
from iii.iii import III
from iii.iii_types import RegisterFunctionFormat, RegisterFunctionInput


def test_register_function_input_model() -> None:
    """RegisterFunctionInput should be constructible with just an id."""
    inp = RegisterFunctionInput(id="demo.fn")
    assert inp.id == "demo.fn"
    assert inp.description is None
    assert inp.request_format is None
    assert inp.response_format is None
    assert inp.metadata is None


def test_register_function_input_with_all_fields() -> None:
    """RegisterFunctionInput should accept all optional fields."""
    req_fmt = RegisterFunctionFormat(name="input", type="object")
    res_fmt = RegisterFunctionFormat(name="output", type="string")
    inp = RegisterFunctionInput(
        id="demo.fn",
        description="A demo function",
        request_format=req_fmt,
        response_format=res_fmt,
        metadata={"version": "1.0"},
    )
    assert inp.id == "demo.fn"
    assert inp.description == "A demo function"
    assert inp.request_format is not None
    assert inp.response_format is not None
    assert inp.metadata == {"version": "1.0"}


# ---------------------------------------------------------------------------
# FakeWs helpers
# ---------------------------------------------------------------------------

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

    async def fake_connect(_: str) -> FakeWebSocket:
        return ws

    monkeypatch.setattr(iii_module.websockets, "connect", fake_connect)
    monkeypatch.setattr("iii.telemetry.init_otel", lambda **kwargs: None)
    monkeypatch.setattr("iii.telemetry.attach_event_loop", lambda loop: None)
    monkeypatch.setattr(iii_module.III, "_register_worker_metadata", lambda self: None)
    return ws


def _make_client() -> III:
    client = III("ws://fake", InitOptions())
    time.sleep(0.05)
    return client


# ---------------------------------------------------------------------------
# Two-arg register_function tests
# ---------------------------------------------------------------------------

def test_register_function_dict_with_request_format(monkeypatch: pytest.MonkeyPatch) -> None:
    """register_function accepts a dict as first arg, with request_format."""
    ws = _patch_ws(monkeypatch)
    client = _make_client()

    req_fmt = RegisterFunctionFormat(
        name="input",
        type="object",
        body=[
            RegisterFunctionFormat(name="name", type="string", required=True),
            RegisterFunctionFormat(name="age", type="number"),
        ],
    )

    async def handler(data: Any) -> Any:
        return data

    client.register_function({"id": "demo.with_args", "request_format": req_fmt}, handler)
    time.sleep(0.02)

    reg_msgs = [m for m in ws.sent if m.get("type") == "registerfunction" and m.get("id") == "demo.with_args"]
    assert len(reg_msgs) == 1

    sent_req_fmt = reg_msgs[0].get("request_format")
    assert sent_req_fmt is not None
    assert sent_req_fmt["name"] == "input"
    assert sent_req_fmt["type"] == "object"
    assert len(sent_req_fmt["body"]) == 2
    assert sent_req_fmt["body"][0]["name"] == "name"
    assert sent_req_fmt["body"][0]["required"] is True

    client.shutdown()


def test_register_function_model_with_both_formats(monkeypatch: pytest.MonkeyPatch) -> None:
    """register_function accepts a RegisterFunctionInput model."""
    ws = _patch_ws(monkeypatch)
    client = _make_client()

    req_fmt = RegisterFunctionFormat(name="input", type="object", body=[
        RegisterFunctionFormat(name="query", type="string", required=True),
    ])
    res_fmt = RegisterFunctionFormat(name="output", type="object", body=[
        RegisterFunctionFormat(name="items", type="array", items=RegisterFunctionFormat(name="item", type="string")),
    ])

    async def handler(data: Any) -> Any:
        return {"items": []}

    func_input = RegisterFunctionInput(
        id="demo.both_formats",
        description="A search function",
        request_format=req_fmt,
        response_format=res_fmt,
        metadata={"version": "1"},
    )
    client.register_function(func_input, handler)
    time.sleep(0.02)

    reg_msgs = [m for m in ws.sent if m.get("type") == "registerfunction" and m.get("id") == "demo.both_formats"]
    assert len(reg_msgs) == 1
    assert reg_msgs[0].get("description") == "A search function"
    assert reg_msgs[0].get("request_format") is not None
    assert reg_msgs[0].get("response_format") is not None
    assert reg_msgs[0]["response_format"]["body"][0]["type"] == "array"
    assert reg_msgs[0]["metadata"] == {"version": "1"}

    client.shutdown()


def test_register_function_dict_minimal(monkeypatch: pytest.MonkeyPatch) -> None:
    """register_function with just {id} and handler — no formats sent."""
    ws = _patch_ws(monkeypatch)
    client = _make_client()

    async def handler(data: Any) -> Any:
        return data

    client.register_function({"id": "demo.minimal"}, handler)
    time.sleep(0.02)

    reg_msgs = [m for m in ws.sent if m.get("type") == "registerfunction" and m.get("id") == "demo.minimal"]
    assert len(reg_msgs) == 1
    assert "request_format" not in reg_msgs[0]
    assert "response_format" not in reg_msgs[0]

    client.shutdown()


def test_register_function_dict_with_http_invocation(monkeypatch: pytest.MonkeyPatch) -> None:
    """register_function with dict + HttpInvocationConfig."""
    ws = _patch_ws(monkeypatch)
    client = _make_client()

    from iii import HttpInvocationConfig

    req_fmt = RegisterFunctionFormat(name="input", type="object", body=[
        RegisterFunctionFormat(name="payload", type="string"),
    ])

    client.register_function(
        {"id": "external::with_format", "request_format": req_fmt},
        HttpInvocationConfig(url="https://example.com/fn", method="POST"),
    )
    time.sleep(0.02)

    reg_msgs = [m for m in ws.sent if m.get("type") == "registerfunction" and m.get("id") == "external::with_format"]
    assert len(reg_msgs) == 1
    assert reg_msgs[0].get("invocation", {}).get("url") == "https://example.com/fn"
    assert reg_msgs[0].get("request_format") is not None
    assert reg_msgs[0]["request_format"]["name"] == "input"

    client.shutdown()


def test_register_function_input_importable_from_top_level() -> None:
    """RegisterFunctionInput and RegisterFunctionFormat should be importable from iii."""
    from iii import RegisterFunctionInput
    from iii.iii_types import RegisterFunctionFormat

    fmt = RegisterFunctionFormat(name="test", type="string")
    assert fmt.name == "test"

    inp = RegisterFunctionInput(id="test.fn", request_format=fmt)
    assert inp.id == "test.fn"
    assert inp.request_format is not None
