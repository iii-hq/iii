"""Integration tests for HTTP API trigger endpoints."""

import asyncio
import json
import time
from pathlib import Path
from urllib.parse import urlencode

import aiohttp
import pytest

from iii import http
from iii.iii import III
from iii.types import HttpRequest, HttpResponse

TEST_ASSETS_DIR = Path(__file__).parent.parent.parent.parent.parent / "test-assets"
TEST_FILE = TEST_ASSETS_DIR / "handbook.pdf"


@pytest.mark.asyncio
async def test_get_endpoint(engine_http_url, iii_client: III):
    """Register a GET endpoint and verify the JSON response."""

    def handler(input_data):
        return {"status_code": 200, "body": {"message": "Hello from GET"}}

    fn_ref = iii_client.register_function({"id": "test.api.get.py"}, handler)
    trigger = iii_client.register_trigger(
        {
            "type": "http",
            "function_id": "test.api.get.py",
            "config": {
                "api_path": "test/py/hello",
                "http_method": "GET",
            },
        }
    )

    time.sleep(0.3)

    async with aiohttp.ClientSession() as session:
        async with session.get(f"{engine_http_url}/test/py/hello") as resp:
            assert resp.status == 200
            data = await resp.json()
            assert data["message"] == "Hello from GET"

    fn_ref.unregister()
    trigger.unregister()


@pytest.mark.asyncio
async def test_post_endpoint_with_body(engine_http_url, iii_client: III):
    """Register a POST endpoint and verify the request body is received."""

    def handler(input_data):
        body = input_data.get("body", {})
        return {
            "status_code": 201,
            "body": {"received": body, "created": True},
        }

    fn_ref = iii_client.register_function({"id": "test.api.post.py"}, handler)
    trigger = iii_client.register_trigger(
        {
            "type": "http",
            "function_id": "test.api.post.py",
            "config": {
                "api_path": "test/py/items",
                "http_method": "POST",
            },
        }
    )

    time.sleep(0.3)

    async with aiohttp.ClientSession() as session:
        async with session.post(
            f"{engine_http_url}/test/py/items",
            json={"name": "test item", "value": 123},
        ) as resp:
            assert resp.status == 201
            data = await resp.json()
            assert data["created"] is True
            assert data["received"]["name"] == "test item"

    fn_ref.unregister()
    trigger.unregister()


@pytest.mark.asyncio
async def test_path_parameters(engine_http_url, iii_client: III):
    """Verify path parameters are extracted correctly."""

    def handler(input_data):
        return {
            "status_code": 200,
            "body": {"id": input_data.get("path_params", {}).get("id")},
        }

    fn_ref = iii_client.register_function({"id": "test.api.getbyid.py"}, handler)
    trigger = iii_client.register_trigger(
        {
            "type": "http",
            "function_id": "test.api.getbyid.py",
            "config": {
                "api_path": "test/py/items/:id",
                "http_method": "GET",
            },
        }
    )

    time.sleep(0.3)

    async with aiohttp.ClientSession() as session:
        async with session.get(f"{engine_http_url}/test/py/items/abc123") as resp:
            assert resp.status == 200
            data = await resp.json()
            assert data["id"] == "abc123"

    fn_ref.unregister()
    trigger.unregister()


@pytest.mark.asyncio
async def test_query_parameters(engine_http_url, iii_client: III):
    """Verify query parameters are passed through."""

    def handler(input_data):
        qp = input_data.get("query_params", {})
        q = qp.get("q")
        limit = qp.get("limit")
        if isinstance(q, list):
            q = q[0]
        if isinstance(limit, list):
            limit = limit[0]
        return {
            "status_code": 200,
            "body": {"query": q, "limit": limit},
        }

    fn_ref = iii_client.register_function({"id": "test.api.search.py"}, handler)
    trigger = iii_client.register_trigger(
        {
            "type": "http",
            "function_id": "test.api.search.py",
            "config": {
                "api_path": "test/py/search",
                "http_method": "GET",
            },
        }
    )

    time.sleep(0.3)

    async with aiohttp.ClientSession() as session:
        async with session.get(f"{engine_http_url}/test/py/search?q=hello&limit=10") as resp:
            assert resp.status == 200
            data = await resp.json()
            assert data["query"] == "hello"
            assert data["limit"] == "10"

    fn_ref.unregister()
    trigger.unregister()


@pytest.mark.asyncio
async def test_custom_status_code(engine_http_url, iii_client: III):
    """Verify a custom HTTP status code is returned."""

    def handler(input_data):
        return {"status_code": 404, "body": {"error": "Not found"}}

    fn_ref = iii_client.register_function({"id": "test.api.notfound.py"}, handler)
    trigger = iii_client.register_trigger(
        {
            "type": "http",
            "function_id": "test.api.notfound.py",
            "config": {
                "api_path": "test/py/missing",
                "http_method": "GET",
            },
        }
    )

    time.sleep(0.3)

    async with aiohttp.ClientSession() as session:
        async with session.get(f"{engine_http_url}/test/py/missing") as resp:
            assert resp.status == 404
            data = await resp.json()
            assert data == {"error": "Not found"}

    fn_ref.unregister()
    trigger.unregister()


@pytest.mark.asyncio
async def test_download_pdf_streaming(engine_http_url, iii_client: III):
    """Stream a PDF file as a download response."""
    if not TEST_FILE.exists():
        pytest.skip("handbook.pdf not found in tests/files")

    original_pdf = TEST_FILE.read_bytes()

    @http
    async def handler(req: HttpRequest, response: HttpResponse):
        await response.status(200)
        await response.headers({"content-type": "application/pdf"})
        await response.writer.write(original_pdf)
        await response.writer.close_async()

    fn_ref = iii_client.register_function({"id": "test.api.download.pdf.py"}, handler)
    trigger = iii_client.register_trigger(
        {
            "type": "http",
            "function_id": "test.api.download.pdf.py",
            "config": {
                "api_path": "test/py/download/pdf",
                "http_method": "GET",
            },
        }
    )

    time.sleep(0.3)

    async with aiohttp.ClientSession() as session:
        async with session.get(f"{engine_http_url}/test/py/download/pdf") as resp:
            assert resp.status == 200
            assert resp.headers.get("content-type") == "application/pdf"
            downloaded = await resp.read()
            assert len(downloaded) == len(original_pdf)
            assert downloaded == original_pdf

    fn_ref.unregister()
    trigger.unregister()


@pytest.mark.asyncio
async def test_upload_pdf_streaming(engine_http_url, iii_client: III):
    """Upload a PDF file via streaming request body."""
    if not TEST_FILE.exists():
        pytest.skip("handbook.pdf not found in tests/files")

    original_pdf = TEST_FILE.read_bytes()

    received_data = bytearray()

    @http
    async def handler(req: HttpRequest, response: HttpResponse):
        nonlocal received_data
        await response.status(200)
        await response.headers({"content-type": "application/json"})

        chunks = []
        async for chunk in req.request_body:
            chunks.append(chunk)

        received_data = bytearray(b"".join(chunks))
        body = json.dumps({"received_size": len(received_data)}).encode("utf-8")
        await response.writer.write(body)
        await response.writer.close_async()

    fn_ref = iii_client.register_function({"id": "test.api.upload.pdf.py"}, handler)
    trigger = iii_client.register_trigger(
        {
            "type": "http",
            "function_id": "test.api.upload.pdf.py",
            "config": {
                "api_path": "test/py/upload/pdf",
                "http_method": "POST",
            },
        }
    )

    time.sleep(0.3)

    async with aiohttp.ClientSession() as session:
        async with session.post(
            f"{engine_http_url}/test/py/upload/pdf",
            headers={"content-type": "application/octet-stream"},
            data=original_pdf,
        ) as resp:
            assert resp.status == 200
            data = await resp.json()
            assert data["received_size"] == len(original_pdf)
            assert bytes(received_data) == original_pdf

    fn_ref.unregister()
    trigger.unregister()


@pytest.mark.asyncio
async def test_sse_streaming(engine_http_url, iii_client: III):
    """Stream Server-Sent Events."""
    events = [
        {"id": "1", "type": "message", "data": "Hello, world!"},
        {"id": "2", "type": "update", "data": json.dumps({"count": 42})},
        {"id": "3", "type": "message", "data": "line one\nline two"},
        {"id": "4", "type": "done", "data": "goodbye"},
    ]

    @http
    async def handler(req: HttpRequest, response: HttpResponse):
        await response.status(200)
        await response.headers(
            {
                "content-type": "text/event-stream",
                "cache-control": "no-cache",
                "connection": "keep-alive",
            }
        )

        for event in events:
            frame = ""
            frame += f"id: {event['id']}\n"
            frame += f"event: {event['type']}\n"
            for line in event["data"].split("\n"):
                frame += f"data: {line}\n"
            frame += "\n"

            await response.writer.write(frame.encode("utf-8"))
            await asyncio.sleep(0.05)

        await response.writer.close_async()

    fn_ref = iii_client.register_function({"id": "test.api.sse.py"}, handler)
    trigger = iii_client.register_trigger(
        {
            "type": "http",
            "function_id": "test.api.sse.py",
            "config": {
                "api_path": "test/py/sse",
                "http_method": "GET",
            },
        }
    )

    time.sleep(0.3)

    async with aiohttp.ClientSession() as session:
        async with session.get(f"{engine_http_url}/test/py/sse") as resp:
            assert resp.status == 200
            assert resp.headers.get("content-type") == "text/event-stream"

            body = await resp.text()
            received_events = []

            for block in body.split("\n\n"):
                if not block.strip():
                    continue
                lines = block.split("\n")
                ev: dict[str, str] = {}
                data_lines: list[str] = []

                for line in lines:
                    if line.startswith("id: "):
                        ev["id"] = line[4:]
                    elif line.startswith("event: "):
                        ev["type"] = line[7:]
                    elif line.startswith("data: "):
                        data_lines.append(line[6:])

                ev["data"] = "\n".join(data_lines)
                received_events.append(ev)

            assert len(received_events) == len(events)
            for i, expected in enumerate(events):
                assert received_events[i]["id"] == expected["id"]
                assert received_events[i]["type"] == expected["type"]
                assert received_events[i]["data"] == expected["data"]

    fn_ref.unregister()
    trigger.unregister()


@pytest.mark.asyncio
async def test_urlencoded_form_data(engine_http_url, iii_client: III):
    """Handle application/x-www-form-urlencoded request."""

    @http
    async def handler(req: HttpRequest, response: HttpResponse):
        raw = await req.request_body.read_all()
        body = raw.decode("utf-8")

        from urllib.parse import parse_qs

        params = parse_qs(body)

        await response.status(200)
        await response.headers({"content-type": "application/json"})
        result = json.dumps(
            {
                "name": params.get("name", [None])[0],
                "email": params.get("email", [None])[0],
                "age": params.get("age", [None])[0],
            }
        ).encode("utf-8")
        await response.writer.write(result)
        await response.writer.close_async()

    fn_ref = iii_client.register_function({"id": "test.api.form.urlencoded.py"}, handler)
    trigger = iii_client.register_trigger(
        {
            "type": "http",
            "function_id": "test.api.form.urlencoded.py",
            "config": {
                "api_path": "test/py/form/urlencoded",
                "http_method": "POST",
            },
        }
    )

    time.sleep(0.3)

    form_body = urlencode({"name": "John Doe", "email": "john@example.com", "age": "30"})

    async with aiohttp.ClientSession() as session:
        async with session.post(
            f"{engine_http_url}/test/py/form/urlencoded",
            headers={"content-type": "application/x-www-form-urlencoded"},
            data=form_body,
        ) as resp:
            assert resp.status == 200
            data = await resp.json()
            assert data["name"] == "John Doe"
            assert data["email"] == "john@example.com"
            assert data["age"] == "30"

    fn_ref.unregister()
    trigger.unregister()


@pytest.mark.asyncio
async def test_multipart_form_data(engine_http_url, iii_client: III):
    """Handle multipart/form-data with file upload."""
    if not TEST_FILE.exists():
        pytest.skip("handbook.pdf not found in tests/files")

    original_pdf = TEST_FILE.read_bytes()

    @http
    async def handler(req: HttpRequest, response: HttpResponse):
        raw = await req.request_body.read_all()
        content_type = req.headers.get("content-type", "")

        boundary_match = None
        for part in content_type.split(";"):
            part = part.strip()
            if part.startswith("boundary="):
                boundary_match = part[len("boundary=") :]

        body_text = raw.decode("utf-8", errors="replace")
        has_title = "Test Document" in body_text
        has_description = "A test upload" in body_text
        has_filename = 'filename="handbook.pdf"' in body_text

        await response.status(200)
        await response.headers({"content-type": "application/json"})
        result = json.dumps(
            {
                "has_boundary": boundary_match is not None and len(boundary_match) > 0,
                "has_title": has_title,
                "has_description": has_description,
                "has_filename": has_filename,
                "body_size": len(raw),
            }
        ).encode("utf-8")
        await response.writer.write(result)
        await response.writer.close_async()

    fn_ref = iii_client.register_function({"id": "test.api.form.multipart.py"}, handler)
    trigger = iii_client.register_trigger(
        {
            "type": "http",
            "function_id": "test.api.form.multipart.py",
            "config": {
                "api_path": "test/py/form/multipart",
                "http_method": "POST",
            },
        }
    )

    time.sleep(0.3)

    form_data = aiohttp.FormData()
    form_data.add_field("title", "Test Document")
    form_data.add_field("description", "A test upload")
    form_data.add_field("file", original_pdf, filename="handbook.pdf", content_type="application/pdf")

    async with aiohttp.ClientSession() as session:
        async with session.post(
            f"{engine_http_url}/test/py/form/multipart",
            data=form_data,
        ) as resp:
            assert resp.status == 200
            data = await resp.json()
            assert data["has_boundary"] is True
            assert data["has_title"] is True
            assert data["has_description"] is True
            assert data["has_filename"] is True
            assert data["body_size"] > len(original_pdf)

    fn_ref.unregister()
    trigger.unregister()
