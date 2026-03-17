"""Integration tests for healthcheck endpoint registration."""

import asyncio

import aiohttp
import pytest

from iii.iii import III


async def get_health(engine_http_url: str) -> tuple[int, dict]:
    async with aiohttp.ClientSession() as session:
        async with session.get(f"{engine_http_url}/health") as resp:
            try:
                body = await resp.json()
            except Exception:
                body = {}
            return resp.status, body


@pytest.mark.asyncio
async def test_register_healthcheck_function_and_trigger(engine_http_url: str, iii_client: III):
    """Registering a healthcheck function and HTTP trigger makes the endpoint live."""

    async def healthcheck_handler(_req):
        return {
            "status_code": 200,
            "body": {
                "status": "healthy",
                "timestamp": "2026-01-01T00:00:00Z",
                "service": "iii-sdk-test",
            },
        }

    fn = iii_client.register_function({"id": "test.healthcheck.py"}, healthcheck_handler)

    status_before, _ = await get_health(engine_http_url)
    assert status_before == 404

    trigger = iii_client.register_trigger({
        "type": "http",
        "function_id": fn.id,
        "config": {"api_path": "health", "http_method": "GET", "description": "Healthcheck endpoint"},
    })

    await asyncio.sleep(0.3)

    async def assert_healthy():
        status, data = await get_health(engine_http_url)
        assert status == 200
        assert data["status"] == "healthy"
        assert data["service"] == "iii-sdk-test"
        assert "timestamp" in data

    deadline = asyncio.get_event_loop().time() + 5.0
    while True:
        try:
            await assert_healthy()
            break
        except AssertionError:
            if asyncio.get_event_loop().time() >= deadline:
                raise
            await asyncio.sleep(0.1)

    fn.unregister()
    trigger.unregister()

    await asyncio.sleep(0.3)

    deadline = asyncio.get_event_loop().time() + 5.0
    while True:
        status_after, _ = await get_health(engine_http_url)
        if status_after == 404:
            break
        if asyncio.get_event_loop().time() >= deadline:
            raise AssertionError(f"Expected 404 after unregister, got {status_after}")
        await asyncio.sleep(0.1)
