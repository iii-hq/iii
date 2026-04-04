"""Integration tests for HTTP middleware execution via the engine."""

import time

import aiohttp
import pytest

from iii.iii import III


@pytest.mark.asyncio
async def test_middleware_continue_to_handler(engine_http_url, iii_client: III):
    """Middleware returns continue, handler runs and responds."""
    middleware_called = {"value": False}

    def mw_handler(req):
        middleware_called["value"] = True
        return {"action": "continue"}

    def handler(req):
        return {"status_code": 200, "body": {"message": "handler reached"}}

    mw_ref = iii_client.register_function({"id": "test.mw.continue.py"}, mw_handler)
    fn_ref = iii_client.register_function({"id": "test.mw.continue.handler.py"}, handler)
    trigger = iii_client.register_trigger(
        {
            "type": "http",
            "function_id": "test.mw.continue.handler.py",
            "config": {
                "api_path": "test/py/mw/continue",
                "http_method": "GET",
                "middleware_function_ids": ["test.mw.continue.py"],
            },
        }
    )

    time.sleep(0.3)

    async with aiohttp.ClientSession() as session:
        async with session.get(f"{engine_http_url}/test/py/mw/continue") as resp:
            assert resp.status == 200
            data = await resp.json()
            assert data["message"] == "handler reached"

    assert middleware_called["value"] is True

    mw_ref.unregister()
    fn_ref.unregister()
    trigger.unregister()


@pytest.mark.asyncio
async def test_middleware_short_circuit(engine_http_url, iii_client: III):
    """Middleware returns respond, handler is skipped."""
    handler_called = {"value": False}

    def mw_handler(req):
        return {
            "action": "respond",
            "response": {
                "status_code": 403,
                "body": {"error": "Forbidden by middleware"},
            },
        }

    def handler(req):
        handler_called["value"] = True
        return {"status_code": 200, "body": {"message": "should not reach"}}

    mw_ref = iii_client.register_function({"id": "test.mw.block.py"}, mw_handler)
    fn_ref = iii_client.register_function({"id": "test.mw.block.handler.py"}, handler)
    trigger = iii_client.register_trigger(
        {
            "type": "http",
            "function_id": "test.mw.block.handler.py",
            "config": {
                "api_path": "test/py/mw/block",
                "http_method": "GET",
                "middleware_function_ids": ["test.mw.block.py"],
            },
        }
    )

    time.sleep(0.3)

    async with aiohttp.ClientSession() as session:
        async with session.get(f"{engine_http_url}/test/py/mw/block") as resp:
            assert resp.status == 403
            data = await resp.json()
            assert data["error"] == "Forbidden by middleware"

    assert handler_called["value"] is False

    mw_ref.unregister()
    fn_ref.unregister()
    trigger.unregister()


@pytest.mark.asyncio
async def test_multiple_middleware_ordering(engine_http_url, iii_client: III):
    """Multiple middleware execute in registration order, then handler."""
    call_order = []

    def mw1(req):
        call_order.append("mw1")
        return {"action": "continue"}

    def mw2(req):
        call_order.append("mw2")
        return {"action": "continue"}

    def handler(req):
        call_order.append("handler")
        return {"status_code": 200, "body": {"order": call_order}}

    mw1_ref = iii_client.register_function({"id": "test.mw.order.first.py"}, mw1)
    mw2_ref = iii_client.register_function({"id": "test.mw.order.second.py"}, mw2)
    fn_ref = iii_client.register_function({"id": "test.mw.order.handler.py"}, handler)
    trigger = iii_client.register_trigger(
        {
            "type": "http",
            "function_id": "test.mw.order.handler.py",
            "config": {
                "api_path": "test/py/mw/order",
                "http_method": "GET",
                "middleware_function_ids": [
                    "test.mw.order.first.py",
                    "test.mw.order.second.py",
                ],
            },
        }
    )

    time.sleep(0.3)

    async with aiohttp.ClientSession() as session:
        async with session.get(f"{engine_http_url}/test/py/mw/order") as resp:
            assert resp.status == 200
            data = await resp.json()
            assert data["order"] == ["mw1", "mw2", "handler"]

    mw1_ref.unregister()
    mw2_ref.unregister()
    fn_ref.unregister()
    trigger.unregister()


@pytest.mark.asyncio
async def test_middleware_receives_request_metadata(engine_http_url, iii_client: III):
    """Middleware receives phase, path_params, query_params, headers, method."""
    received = {"req": None}

    def mw_handler(req):
        received["req"] = req
        return {"action": "continue"}

    def handler(req):
        return {"status_code": 200, "body": {"ok": True}}

    mw_ref = iii_client.register_function({"id": "test.mw.meta.py"}, mw_handler)
    fn_ref = iii_client.register_function({"id": "test.mw.meta.handler.py"}, handler)
    trigger = iii_client.register_trigger(
        {
            "type": "http",
            "function_id": "test.mw.meta.handler.py",
            "config": {
                "api_path": "test/py/mw/meta/:id",
                "http_method": "GET",
                "middleware_function_ids": ["test.mw.meta.py"],
            },
        }
    )

    time.sleep(0.3)

    async with aiohttp.ClientSession() as session:
        async with session.get(f"{engine_http_url}/test/py/mw/meta/42?q=hello") as resp:
            assert resp.status == 200

    req = received["req"]
    assert req is not None
    assert req["phase"] == "preHandler"
    assert req["request"]["method"] == "GET"
    assert req["request"]["path_params"]["id"] == "42"
    assert req["request"]["query_params"]["q"] == "hello"

    mw_ref.unregister()
    fn_ref.unregister()
    trigger.unregister()


@pytest.mark.asyncio
async def test_no_middleware_regression(engine_http_url, iii_client: III):
    """Handler works normally without any middleware (regression test)."""

    def handler(req):
        return {"status_code": 200, "body": {"message": "no middleware"}}

    fn_ref = iii_client.register_function({"id": "test.mw.none.py"}, handler)
    trigger = iii_client.register_trigger(
        {
            "type": "http",
            "function_id": "test.mw.none.py",
            "config": {
                "api_path": "test/py/mw/none",
                "http_method": "GET",
            },
        }
    )

    time.sleep(0.3)

    async with aiohttp.ClientSession() as session:
        async with session.get(f"{engine_http_url}/test/py/mw/none") as resp:
            assert resp.status == 200
            data = await resp.json()
            assert data["message"] == "no middleware"

    fn_ref.unregister()
    trigger.unregister()
