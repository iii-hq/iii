import httpx
import pytest
from iii_observability import execute_traced_request


@pytest.mark.asyncio
async def test_execute_traced_request_returns_response(httpx_mock):
    httpx_mock.add_response(url="https://example.com/api", text="ok", status_code=200)
    async with httpx.AsyncClient() as client:
        req = client.build_request("GET", "https://example.com/api")
        res = await execute_traced_request(client, req)
        assert res.status_code == 200
        assert res.text == "ok"


@pytest.mark.asyncio
async def test_execute_traced_request_records_error_status(httpx_mock):
    httpx_mock.add_response(url="https://example.com/api", text="bad", status_code=500)
    async with httpx.AsyncClient() as client:
        req = client.build_request("GET", "https://example.com/api")
        res = await execute_traced_request(client, req)
        assert res.status_code == 500
