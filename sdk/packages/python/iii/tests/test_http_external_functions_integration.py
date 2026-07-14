"""Tests for HTTP external functions.

Unit tests use FakeWebSocket; integration tests require a running III engine with HttpFunctionsModule.
"""

import asyncio
import json
import os
import random
import time
from typing import Any

import pytest
from iii_helpers.http import HttpAuthBearer, HttpInvocationConfig

from iii import InitOptions
from iii.iii import III


def _unique_function_id(prefix: str) -> str:
    return f"{prefix}::{int(time.time())}::{random.random():.10f}".replace(".", "")


class WebhookProbe:
    def __init__(self) -> None:
        self._received: list[dict[str, Any]] = []
        self._waiter: asyncio.Future[dict[str, Any]] | None = None
        self._server: asyncio.Server | None = None
        self._port = 0

    async def start(self) -> None:
        self._server = await asyncio.start_server(
            self._handle_request,
            "127.0.0.1",
            0,
        )
        for sock in self._server.sockets or []:
            self._port = sock.getsockname()[1]
            break

    async def _handle_request(
        self,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
    ) -> None:
        data = b""
        while True:
            chunk = await reader.read(4096)
            if not chunk:
                break
            data += chunk
            if b"\r\n\r\n" in data or b"\n\n" in data:
                break

        lines = data.decode().split("\r\n") if data else []
        method = "POST"
        if lines:
            parts = lines[0].split()
            if len(parts) >= 1:
                method = parts[0]
        path = "/"
        if lines and " " in lines[0]:
            path = lines[0].split(" ")[1].split("?")[0]

        # Parse headers from raw HTTP request lines.
        # Headers are between the request line (index 0) and the first empty line.
        headers: dict[str, str] = {}
        for line in lines[1:]:
            if line == "":
                break
            if ":" in line:
                key, _, value = line.partition(":")
                headers[key.strip()] = value.strip()

        body = b""
        if b"\r\n\r\n" in data:
            body = data.split(b"\r\n\r\n", 1)[1]
        elif b"\n\n" in data:
            body = data.split(b"\n\n", 1)[1]

        try:
            body_json = json.loads(body.decode()) if body else None
        except Exception:
            body_json = body.decode() if body else None

        captured = {"method": method, "url": path, "body": body_json, "headers": headers}
        if self._waiter and not self._waiter.done():
            self._waiter.set_result(captured)
        else:
            self._received.append(captured)

        writer.write(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n")
        writer.write(b'{"ok":true}')
        await writer.drain()
        writer.close()
        await writer.wait_closed()

    async def close(self) -> None:
        if self._server:
            self._server.close()
            await self._server.wait_closed()
            self._server = None

    def url(self, path: str = "/webhook") -> str:
        return f"http://127.0.0.1:{self._port}{path}"

    async def wait_for_webhook(self, timeout: float = 7.0) -> dict[str, Any]:
        if self._received:
            return self._received.pop(0)
        self._waiter = asyncio.get_running_loop().create_future()
        try:
            return await asyncio.wait_for(self._waiter, timeout=timeout)
        finally:
            self._waiter = None

    async def wait_for_webhook_or_none(self, timeout: float = 2.0) -> dict[str, Any] | None:
        """Wait for a webhook, returning None if nothing arrives within timeout."""
        try:
            return await self.wait_for_webhook(timeout=timeout)
        except (asyncio.TimeoutError, TimeoutError):
            return None


# ---------------------------------------------------------------------------
# Helpers for FakeWs-based unit tests
# ---------------------------------------------------------------------------


def _make_fake_ws_env(monkeypatch: pytest.MonkeyPatch) -> list[dict[str, Any]]:
    """Set up a FakeWs monkeypatch and return the list that collects sent messages."""
    from types import SimpleNamespace

    import iii.iii as iii_module

    sent: list[dict[str, Any]] = []

    class FakeWs:
        state = SimpleNamespace(name="OPEN")

        async def send(self, payload: str) -> None:
            sent.append(json.loads(payload))

        async def close(self) -> None:
            self.state = SimpleNamespace(name="CLOSED")

        def __aiter__(self):
            return self

        async def __anext__(self):
            raise StopAsyncIteration

    async def fake_connect(_: str, **kwargs: object) -> FakeWs:
        return FakeWs()

    monkeypatch.setattr(iii_module.websockets, "connect", fake_connect)
    monkeypatch.setattr("iii_helpers.observability.telemetry.init_otel", lambda **kwargs: None)
    monkeypatch.setattr("iii_helpers.observability.telemetry.attach_event_loop", lambda loop: None)
    monkeypatch.setattr(iii_module.III, "_register_worker_metadata", lambda self: None)
    return sent


def _make_connected_client() -> III:
    """Create an III client connected via FakeWs (caller must have monkeypatched already)."""
    client = III("ws://fake", InitOptions())
    time.sleep(0.05)
    return client


# ---------------------------------------------------------------------------
# Helpers for integration tests
# ---------------------------------------------------------------------------


def _make_integration_client() -> III:
    """Create a real III client connected to the engine; skips if unavailable."""
    ws_url = os.environ.get("III_URL", "ws://localhost:49199")
    client = III(ws_url, InitOptions(reconnection_config=None))
    client._wait_until_connected()
    time.sleep(0.1)

    try:
        client.trigger({"function_id": "engine::functions::list", "payload": {}})
    except Exception:
        client.shutdown()
        pytest.skip("III engine not available")

    return client


async def _invoke(client: III, function_id: str, payload: dict[str, Any]) -> Any:
    """Directly invoke an HTTP external function without blocking the test loop.

    ``client.trigger`` is synchronous and blocks the caller until the engine's
    webhook POST round-trips. Since the WebhookProbe server runs on this same
    event loop, calling it inline would deadlock -- so we run it on a worker
    thread and let the loop keep serving the probe.
    """
    loop = asyncio.get_running_loop()
    return await loop.run_in_executor(
        None,
        lambda: client.trigger({"function_id": function_id, "payload": payload}),
    )


# ===========================================================================
# Unit tests (FakeWs)
# ===========================================================================


def test_register_http_function_sends_invocation_message(monkeypatch: pytest.MonkeyPatch) -> None:
    sent = _make_fake_ws_env(monkeypatch)
    client = _make_connected_client()

    config = HttpInvocationConfig(url="https://example.com/invoke", method="POST", timeout_ms=3000)
    ref = client.register_function("external::my_lambda", config)
    time.sleep(0.02)

    assert ref.id == "external::my_lambda"
    reg_fn = [m for m in sent if m.get("type") == "registerfunction" and m.get("id") == "external::my_lambda"]
    assert len(reg_fn) == 1
    assert reg_fn[0].get("invocation", {}).get("url") == "https://example.com/invoke"
    assert reg_fn[0].get("invocation", {}).get("method") == "POST"

    ref.unregister()
    time.sleep(0.05)
    unreg = [m for m in sent if m.get("type") == "unregisterfunction" and m.get("id") == "external::my_lambda"]
    assert len(unreg) == 1, f"Expected 1 unregister message, got {len(unreg)}. Sent: {sent}"

    client.shutdown()


def test_register_http_function_with_all_config_options(monkeypatch: pytest.MonkeyPatch) -> None:
    sent = _make_fake_ws_env(monkeypatch)
    client = _make_connected_client()

    config = HttpInvocationConfig(
        url="https://api.example.com/handler",
        method="PUT",
        timeout_ms=5000,
        headers={"X-Custom-Header": "test-value", "X-Another": "123"},
        auth=HttpAuthBearer(token_key="MY_SECRET_TOKEN"),
    )
    ref = client.register_function("external::full_config", config)
    time.sleep(0.02)

    assert ref.id == "external::full_config"

    reg_msgs = [m for m in sent if m.get("type") == "registerfunction" and m.get("id") == "external::full_config"]
    assert len(reg_msgs) == 1

    invocation = reg_msgs[0].get("invocation", {})
    assert invocation["url"] == "https://api.example.com/handler"
    assert invocation["method"] == "PUT"
    assert invocation["timeout_ms"] == 5000
    assert invocation["headers"] == {"X-Custom-Header": "test-value", "X-Another": "123"}
    assert invocation["auth"]["type"] == "bearer"
    assert invocation["auth"]["token_key"] == "MY_SECRET_TOKEN"

    client.shutdown()


def test_unregister_removes_function_from_sent_messages(monkeypatch: pytest.MonkeyPatch) -> None:
    sent = _make_fake_ws_env(monkeypatch)
    client = _make_connected_client()

    config = HttpInvocationConfig(url="https://example.com/fn", method="POST")
    ref = client.register_function("external::to_remove", config)
    time.sleep(0.02)

    # Verify registration was sent.
    reg_msgs = [m for m in sent if m.get("type") == "registerfunction" and m.get("id") == "external::to_remove"]
    assert len(reg_msgs) == 1

    # Unregister.
    ref.unregister()
    time.sleep(0.05)

    # Verify unregister was sent with correct id.
    unreg_msgs = [m for m in sent if m.get("type") == "unregisterfunction" and m.get("id") == "external::to_remove"]
    assert len(unreg_msgs) == 1
    assert unreg_msgs[0]["id"] == "external::to_remove"

    # Verify the function is removed from internal tracking so it would not be
    # re-registered on reconnect.
    assert "external::to_remove" not in client._functions

    client.shutdown()


# ===========================================================================
# Integration tests (require running III engine)
# ===========================================================================


@pytest.mark.asyncio
async def test_delivers_events_to_external_http_function() -> None:
    client = _make_integration_client()

    probe = WebhookProbe()
    await probe.start()

    function_id = _unique_function_id("test::http_external::target")
    payload = {"hello": "world", "count": 1}
    http_fn = None

    try:
        http_fn = client.register_function(
            function_id,
            HttpInvocationConfig(url=probe.url(), method="POST", timeout_ms=3000),
        )
        time.sleep(0.5)

        await _invoke(client, function_id, payload)

        webhook = await probe.wait_for_webhook(7.0)

        assert webhook["method"] == "POST"
        assert webhook["url"] == "/webhook"
        # Direct invocation injects caller metadata (e.g. _caller_worker_id) into
        # the POSTed body, so assert the expected payload is a subset.
        assert payload.items() <= webhook["body"].items()
    finally:
        if http_fn:
            http_fn.unregister()
        await probe.close()
        client.shutdown()


@pytest.mark.asyncio
async def test_registers_and_unregisters_external_function() -> None:
    client = _make_integration_client()

    # Use a real local server so the engine's URL validator accepts the registration.
    probe = WebhookProbe()
    await probe.start()

    function_id = _unique_function_id("test::http_external::list_check")
    http_fn = None

    try:
        http_fn = client.register_function(
            function_id,
            HttpInvocationConfig(url=probe.url(), method="POST", timeout_ms=3000),
        )
        time.sleep(0.5)

        # Verify the function appears in the engine function list (with retries for timing).
        found = False
        for _ in range(10):
            result = client.trigger({"function_id": "engine::functions::list", "payload": {}})
            function_ids = [f["function_id"] for f in result.get("functions", [])]
            if function_id in function_ids:
                found = True
                break
            time.sleep(0.3)
        assert found, f"{function_id} not found in {function_ids}"

        # Unregister.
        http_fn.unregister()
        http_fn = None
        time.sleep(0.5)

        # Verify the function is gone (with retries for timing).
        gone = False
        for _ in range(10):
            result = client.trigger({"function_id": "engine::functions::list", "payload": {}})
            function_ids = [f["function_id"] for f in result.get("functions", [])]
            if function_id not in function_ids:
                gone = True
                break
            time.sleep(0.3)
        assert gone, f"{function_id} still found after unregister"
    finally:
        if http_fn:
            http_fn.unregister()
        await probe.close()
        client.shutdown()


@pytest.mark.asyncio
async def test_delivers_events_with_custom_headers() -> None:
    client = _make_integration_client()

    probe = WebhookProbe()
    await probe.start()

    function_id = _unique_function_id("test::http_external::custom_headers")
    payload = {"event": "custom_header_test"}
    http_fn = None

    try:
        http_fn = client.register_function(
            function_id,
            HttpInvocationConfig(
                url=probe.url(),
                method="POST",
                timeout_ms=3000,
                headers={"X-Custom-Header": "test-value", "X-Another": "123"},
            ),
        )
        time.sleep(0.5)

        await _invoke(client, function_id, payload)

        webhook = await probe.wait_for_webhook(7.0)

        assert webhook["method"] == "POST"
        # Direct invocation injects caller metadata (e.g. _caller_worker_id) into
        # the POSTed body, so assert the expected payload is a subset.
        assert payload.items() <= webhook["body"].items()

        # Verify custom headers were forwarded by the engine.
        # Header keys may be lowercased by the HTTP client.
        received_headers = {k.lower(): v for k, v in webhook["headers"].items()}
        assert received_headers.get("x-custom-header") == "test-value", (
            f"Expected x-custom-header=test-value, got headers: {received_headers}"
        )
        assert received_headers.get("x-another") == "123", f"Expected x-another=123, got headers: {received_headers}"
    finally:
        if http_fn:
            http_fn.unregister()
        await probe.close()
        client.shutdown()


@pytest.mark.asyncio
async def test_delivers_events_to_multiple_external_functions() -> None:
    client = _make_integration_client()

    probe_a = WebhookProbe()
    probe_b = WebhookProbe()
    await probe_a.start()
    await probe_b.start()

    fn_id_a = _unique_function_id("test::http_external::multi_a")
    fn_id_b = _unique_function_id("test::http_external::multi_b")
    payload_a = {"source": "topic_a", "value": 1}
    payload_b = {"source": "topic_b", "value": 2}

    http_fn_a = None
    http_fn_b = None

    try:
        http_fn_a = client.register_function(
            fn_id_a,
            HttpInvocationConfig(url=probe_a.url("/hook_a"), method="POST", timeout_ms=3000),
        )
        http_fn_b = client.register_function(
            fn_id_b,
            HttpInvocationConfig(url=probe_b.url("/hook_b"), method="POST", timeout_ms=3000),
        )
        time.sleep(0.5)

        await _invoke(client, fn_id_a, payload_a)
        await _invoke(client, fn_id_b, payload_b)

        webhook_a = await probe_a.wait_for_webhook(7.0)
        webhook_b = await probe_b.wait_for_webhook(7.0)

        # Each probe should receive only its own function's event. Direct
        # invocation injects caller metadata into the body, so assert subset.
        assert payload_a.items() <= webhook_a["body"].items(), f"probe_a got wrong body: {webhook_a['body']}"
        assert webhook_a["url"] == "/hook_a"

        assert payload_b.items() <= webhook_b["body"].items(), f"probe_b got wrong body: {webhook_b['body']}"
        assert webhook_b["url"] == "/hook_b"
    finally:
        if http_fn_a:
            http_fn_a.unregister()
        if http_fn_b:
            http_fn_b.unregister()
        await probe_a.close()
        await probe_b.close()
        client.shutdown()


@pytest.mark.asyncio
async def test_stops_delivering_after_unregister() -> None:
    client = _make_integration_client()

    probe = WebhookProbe()
    await probe.start()

    function_id = _unique_function_id("test::http_external::stop_deliver")
    payload_before = {"phase": "before_unregister"}
    payload_after = {"phase": "after_unregister"}
    http_fn = None

    try:
        http_fn = client.register_function(
            function_id,
            HttpInvocationConfig(url=probe.url(), method="POST", timeout_ms=3000),
        )
        time.sleep(0.5)

        # First invocation -- should be delivered.
        await _invoke(client, function_id, payload_before)
        webhook = await probe.wait_for_webhook(7.0)
        # Direct invocation injects caller metadata (e.g. _caller_worker_id) into
        # the POSTed body, so assert the expected payload is a subset.
        assert payload_before.items() <= webhook["body"].items()

        # Unregister the function.
        http_fn.unregister()
        http_fn = None
        time.sleep(0.5)

        # Second invocation -- the function is gone, so the trigger is rejected
        # and nothing reaches the webhook.
        try:
            await _invoke(client, function_id, payload_after)
        except Exception:
            pass
        no_delivery = await probe.wait_for_webhook_or_none(timeout=2.0)
        assert no_delivery is None, f"Expected no delivery after unregister, but got: {no_delivery}"
    finally:
        if http_fn:
            http_fn.unregister()
        await probe.close()
        client.shutdown()


@pytest.mark.asyncio
async def test_delivers_with_put_method() -> None:
    client = _make_integration_client()

    probe = WebhookProbe()
    await probe.start()

    function_id = _unique_function_id("test::http_external::put_method")
    payload = {"action": "update", "id": 42}
    http_fn = None

    try:
        http_fn = client.register_function(
            function_id,
            HttpInvocationConfig(url=probe.url(), method="PUT", timeout_ms=3000),
        )
        time.sleep(0.5)

        await _invoke(client, function_id, payload)

        webhook = await probe.wait_for_webhook(7.0)

        assert webhook["method"] == "PUT", f"Expected PUT, got {webhook['method']}"
        # Direct invocation injects caller metadata (e.g. _caller_worker_id) into
        # the POSTed body, so assert the expected payload is a subset.
        assert payload.items() <= webhook["body"].items()
    finally:
        if http_fn:
            http_fn.unregister()
        await probe.close()
        client.shutdown()
