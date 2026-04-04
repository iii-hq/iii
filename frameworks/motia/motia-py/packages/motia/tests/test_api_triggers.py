# motia/tests/test_api_triggers.py
"""Integration tests for API triggers (HTTP endpoints)."""

import json
import time

import httpx
import pytest

try:
    from iii import HttpResponse
except ImportError:
    HttpResponse = None  # type: ignore[assignment,misc]

from tests.conftest import flush_bridge_queue

pytestmark = pytest.mark.integration


@pytest.mark.asyncio
async def test_api_trigger_get_endpoint(bridge, api_url):
    """Test registering a GET endpoint via API trigger."""

    def get_handler(data):
        return {
            "status_code": 200,
            "body": {"message": "Hello from GET"},
        }

    bridge.register_function({"id": "test.api.get"}, get_handler)
    bridge.register_trigger(
        {
            "type": "http",
            "function_id": "test.api.get",
            "config": {
                "api_path": "test/hello",
                "http_method": "GET",
            },
        }
    )

    flush_bridge_queue(bridge)
    time.sleep(0.3)

    async with httpx.AsyncClient() as client:
        response = await client.get(f"{api_url}/test/hello")

    assert response.status_code == 200
    assert response.json() == {"message": "Hello from GET"}


@pytest.mark.asyncio
async def test_api_trigger_post_with_body(bridge, api_url):
    """Test POST endpoint with request body."""

    def post_handler(data):
        body = data.get("body", {})
        return {
            "status_code": 201,
            "body": {"received": body, "created": True},
        }

    bridge.register_function({"id": "test.api.post"}, post_handler)
    bridge.register_trigger(
        {
            "type": "http",
            "function_id": "test.api.post",
            "config": {
                "api_path": "test/items",
                "http_method": "POST",
            },
        }
    )

    flush_bridge_queue(bridge)
    time.sleep(0.3)

    async with httpx.AsyncClient() as client:
        response = await client.post(
            f"{api_url}/test/items",
            json={"name": "test item", "value": 123},
        )

    assert response.status_code == 201
    data = response.json()
    assert data["created"] is True
    assert data["received"]["name"] == "test item"


@pytest.mark.asyncio
async def test_api_trigger_path_params(bridge, api_url):
    """Test endpoint with path parameters."""

    def get_by_id_handler(data):
        path_params = data.get("path_params", {})
        return {
            "status_code": 200,
            "body": {"id": path_params.get("id")},
        }

    bridge.register_function({"id": "test.api.getById"}, get_by_id_handler)
    bridge.register_trigger(
        {
            "type": "http",
            "function_id": "test.api.getById",
            "config": {
                "api_path": "test/items/:id",
                "http_method": "GET",
            },
        }
    )

    flush_bridge_queue(bridge)
    time.sleep(0.3)

    async with httpx.AsyncClient() as client:
        response = await client.get(f"{api_url}/test/items/abc123")

    assert response.status_code == 200
    assert response.json() == {"id": "abc123"}


@pytest.mark.asyncio
async def test_api_trigger_query_params(bridge, api_url):
    """Test endpoint with query parameters."""

    def search_handler(data):
        query_params = data.get("query_params", {})
        return {
            "status_code": 200,
            "body": {"query": query_params.get("q"), "limit": query_params.get("limit")},
        }

    bridge.register_function({"id": "test.api.search"}, search_handler)
    bridge.register_trigger(
        {
            "type": "http",
            "function_id": "test.api.search",
            "config": {
                "api_path": "test/search",
                "http_method": "GET",
            },
        }
    )

    flush_bridge_queue(bridge)
    time.sleep(0.3)

    async with httpx.AsyncClient() as client:
        response = await client.get(f"{api_url}/test/search?q=hello&limit=10")

    assert response.status_code == 200
    data = response.json()
    assert data["query"] == "hello"
    assert data["limit"] == "10"


@pytest.mark.asyncio
async def test_api_trigger_custom_status_code(bridge, api_url):
    """Test returning custom status codes."""

    def not_found_handler(data):
        return {
            "status_code": 404,
            "body": {"error": "Not found"},
        }

    bridge.register_function({"id": "test.api.notfound"}, not_found_handler)
    bridge.register_trigger(
        {
            "type": "http",
            "function_id": "test.api.notfound",
            "config": {
                "api_path": "test/missing",
                "http_method": "GET",
            },
        }
    )

    flush_bridge_queue(bridge)
    time.sleep(0.3)

    async with httpx.AsyncClient() as client:
        response = await client.get(f"{api_url}/test/missing")

    assert response.status_code == 404
    assert response.json() == {"error": "Not found"}


@pytest.mark.asyncio
@pytest.mark.skipif(HttpResponse is None, reason="iii-sdk does not export HttpResponse (needs >= 0.3.0)")
async def test_streaming_response_via_channels(bridge, api_url):
    """Test streaming a JSON response via channels."""
    function_id = f"test.api.stream.{int(time.time() * 1000)}"

    async def stream_handler(req):
        response_writer = req.get("response") if isinstance(req, dict) else getattr(req, "response", None)
        response = HttpResponse(response_writer)
        await response.status(200)
        await response.headers({"content-type": "application/json"})
        response.writer.stream.write(json.dumps({"streamed": True, "message": "hello from stream"}).encode("utf-8"))
        response.close()

    bridge.register_function({"id": function_id}, stream_handler)
    bridge.register_trigger(
        {
            "type": "http",
            "function_id": function_id,
            "config": {"api_path": "test/stream/response", "http_method": "GET"},
        }
    )

    flush_bridge_queue(bridge)
    time.sleep(0.3)

    async with httpx.AsyncClient() as client:
        response = await client.get(f"{api_url}/test/stream/response")

    assert response.status_code == 200
    assert response.headers.get("content-type") == "application/json"
    data = response.json()
    assert data == {"streamed": True, "message": "hello from stream"}


@pytest.mark.asyncio
@pytest.mark.skipif(HttpResponse is None, reason="iii-sdk does not export HttpResponse (needs >= 0.3.0)")
async def test_streaming_request_body_via_channels(bridge, api_url):
    """Test reading a streamed request body via channels."""
    function_id = f"test.api.stream.upload.{int(time.time() * 1000)}"

    async def upload_handler(req):
        response_writer = req.get("response") if isinstance(req, dict) else getattr(req, "response", None)
        request_body = req.get("request_body") if isinstance(req, dict) else getattr(req, "request_body", None)

        chunks: list[bytes] = []
        async for chunk in request_body.stream:
            if isinstance(chunk, bytes):
                chunks.append(chunk)
            else:
                chunks.append(chunk.encode("utf-8"))
        body = b"".join(chunks).decode("utf-8")

        response = HttpResponse(response_writer)
        await response.status(200)
        await response.headers({"content-type": "application/json"})
        response.writer.stream.write(json.dumps({"received": body, "size": len(body)}).encode("utf-8"))
        response.close()

    bridge.register_function({"id": function_id}, upload_handler)
    bridge.register_trigger(
        {
            "type": "http",
            "function_id": function_id,
            "config": {"api_path": "test/stream/upload", "http_method": "POST"},
        }
    )

    flush_bridge_queue(bridge)
    time.sleep(0.3)

    payload = json.dumps({"data": "streamed content", "items": [1, 2, 3]})
    async with httpx.AsyncClient() as client:
        response = await client.post(
            f"{api_url}/test/stream/upload",
            content=payload.encode("utf-8"),
            headers={"content-type": "application/octet-stream"},
        )

    assert response.status_code == 200
    data = response.json()
    assert data["received"] == payload
    assert data["size"] == len(payload)


@pytest.mark.asyncio
@pytest.mark.skipif(HttpResponse is None, reason="iii-sdk does not export HttpResponse (needs >= 0.3.0)")
async def test_multipart_form_data_via_channels(bridge, api_url):
    """Test multipart/form-data with file upload via channels."""
    function_id = f"test.api.multipart.{int(time.time() * 1000)}"

    async def multipart_handler(req):
        response_writer = req.get("response") if isinstance(req, dict) else getattr(req, "response", None)
        request_body = req.get("request_body") if isinstance(req, dict) else getattr(req, "request_body", None)
        headers = req.get("headers", {}) if isinstance(req, dict) else getattr(req, "headers", {})

        chunks: list[bytes] = []
        async for chunk in request_body.stream:
            if isinstance(chunk, bytes):
                chunks.append(chunk)
            else:
                chunks.append(chunk.encode("utf-8"))
        raw_body = b"".join(chunks)

        content_type = headers.get("content-type", "")
        if isinstance(content_type, list):
            content_type = content_type[0] if content_type else ""

        import re

        boundary_match = re.search(r"boundary=([^\s;]+)", content_type)
        body_text = raw_body.decode("utf-8", errors="replace")

        response = HttpResponse(response_writer)
        await response.status(200)
        await response.headers({"content-type": "application/json"})
        response.writer.stream.write(
            json.dumps(
                {
                    "has_boundary": bool(boundary_match and len(boundary_match.group(1)) > 0),
                    "has_title": "Test Document" in body_text,
                    "has_description": "A test upload" in body_text,
                    "has_file_content": "fake file content" in body_text,
                    "body_size": len(raw_body),
                }
            ).encode("utf-8")
        )
        response.close()

    bridge.register_function({"id": function_id}, multipart_handler)
    bridge.register_trigger(
        {
            "type": "http",
            "function_id": function_id,
            "config": {"api_path": "test/form/multipart", "http_method": "POST"},
        }
    )

    flush_bridge_queue(bridge)
    time.sleep(0.3)

    boundary = "----TestBoundary12345"
    body_parts = [
        f"--{boundary}\r\n" 'Content-Disposition: form-data; name="title"\r\n\r\n' "Test Document\r\n",
        f"--{boundary}\r\n" 'Content-Disposition: form-data; name="description"\r\n\r\n' "A test upload\r\n",
        f"--{boundary}\r\n"
        'Content-Disposition: form-data; name="file"; filename="test.txt"\r\n'
        "Content-Type: text/plain\r\n\r\n"
        "fake file content\r\n",
        f"--{boundary}--\r\n",
    ]
    multipart_body = "".join(body_parts).encode("utf-8")

    async with httpx.AsyncClient() as client:
        response = await client.post(
            f"{api_url}/test/form/multipart",
            content=multipart_body,
            headers={"content-type": f"multipart/form-data; boundary={boundary}"},
        )

    assert response.status_code == 200
    data = response.json()
    assert data["has_boundary"] is True
    assert data["has_title"] is True
    assert data["has_description"] is True
    assert data["has_file_content"] is True
    assert data["body_size"] > 0


@pytest.mark.asyncio
@pytest.mark.skipif(HttpResponse is None, reason="iii-sdk does not export HttpResponse (needs >= 0.3.0)")
async def test_data_channels_worker_to_worker(bridge, api_url):
    """Test data channels for worker-to-worker streaming."""
    processor_id = f"test.channel.processor.{int(time.time() * 1000)}"
    sender_id = f"test.channel.sender.{int(time.time() * 1000)}"

    async def processor_handler(input_data):
        label = input_data.get("label", "") if isinstance(input_data, dict) else getattr(input_data, "label", "")
        reader = input_data.get("reader") if isinstance(input_data, dict) else getattr(input_data, "reader", None)

        chunks: list[bytes] = []
        async for chunk in reader.stream:
            if isinstance(chunk, bytes):
                chunks.append(chunk)
            else:
                chunks.append(chunk.encode("utf-8"))

        records = json.loads(b"".join(chunks).decode("utf-8"))
        total = sum(r["value"] for r in records)

        return {"label": label, "count": len(records), "sum": total}

    async def sender_handler(input_data):
        records = input_data.get("records", []) if isinstance(input_data, dict) else getattr(input_data, "records", [])
        channel = await bridge.create_channel_async()
        payload = json.dumps(records).encode("utf-8")
        await channel.writer.write(payload)
        await channel.writer.close_async()

        result = await bridge.trigger_async(
            {
                "function_id": processor_id,
                "payload": {"label": "test-batch", "reader": channel.reader_ref.__dict__},
            }
        )
        return result

    bridge.register_function({"id": processor_id}, processor_handler)
    bridge.register_function({"id": sender_id}, sender_handler)

    flush_bridge_queue(bridge)
    time.sleep(0.3)

    records = [{"value": 10}, {"value": 20}, {"value": 30}]
    result = bridge.trigger({"function_id": sender_id, "payload": {"records": records}})

    assert result["label"] == "test-batch"
    assert result["count"] == 3
    assert result["sum"] == 60
