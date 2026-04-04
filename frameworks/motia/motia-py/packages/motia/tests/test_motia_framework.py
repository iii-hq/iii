# motia/tests/test_motia_framework.py
"""Integration tests for the Motia framework layer."""

import time
import uuid
from unittest.mock import patch

import httpx
import pytest

from tests.conftest import flush_bridge_queue

pytestmark = pytest.mark.integration


@pytest.fixture
def patch_motia_bridge(bridge):
    """Patch the motia bridge to use the test bridge."""
    with patch("motia.runtime.get_instance", return_value=bridge):
        yield bridge


@pytest.mark.asyncio
async def test_motia_api_step(bridge, api_url, patch_motia_bridge):
    """Test registering and invoking an API step via Motia."""
    from motia import Motia, http
    from motia.types import ApiRequest, ApiResponse, FlowContext

    motia = Motia()
    endpoint_path = f"motia/test/{uuid.uuid4().hex[:8]}"

    # Define a step config
    config = {
        "name": "test-api-step",
        "description": "Test API step",
        "triggers": [http("POST", f"/{endpoint_path}", body_schema={"type": "object"})],
    }

    async def handler(input_data: ApiRequest, context: FlowContext) -> ApiResponse:
        body = input_data.body if hasattr(input_data, "body") else input_data.get("body", {})
        return ApiResponse(
            status=200,
            body={"received": body, "processed": True},
        )

    # Add step to Motia runtime
    motia.add_step(config, "test_step.py", handler)

    flush_bridge_queue(bridge)
    time.sleep(0.5)

    # Make HTTP request
    async with httpx.AsyncClient() as client:
        response = await client.post(
            f"{api_url}/{endpoint_path}",
            json={"data": "hello"},
        )

    assert response.status_code == 200
    data = response.json()
    assert data["processed"] is True


@pytest.mark.asyncio
async def test_motia_step_with_query_params(bridge, api_url, patch_motia_bridge):
    """Test a step that receives query parameters."""
    from motia import Motia, http
    from motia.types import ApiRequest, ApiResponse, FlowContext

    motia = Motia()
    endpoint_path = f"motia/search/{uuid.uuid4().hex[:8]}"

    config = {
        "name": "test-query-params-step",
        "description": "Test step with query params",
        "triggers": [http("GET", f"/{endpoint_path}")],
    }

    async def handler(input_data: ApiRequest, context: FlowContext) -> ApiResponse:
        q = input_data.query_params.get("q", "")
        limit = input_data.query_params.get("limit", "10")
        return ApiResponse(
            status=200,
            body={"query": q, "limit": limit},
        )

    motia.add_step(config, "test_query_params_step.py", handler)

    flush_bridge_queue(bridge)
    time.sleep(0.3)

    async with httpx.AsyncClient() as client:
        response = await client.get(
            f"{api_url}/{endpoint_path}?q=hello&limit=20",
        )

    assert response.status_code == 200
    data = response.json()
    assert data["query"] == "hello"
    assert data["limit"] == "20"


@pytest.mark.asyncio
async def test_motia_api_with_path_params(bridge, api_url, patch_motia_bridge):
    """Test API step with path parameters."""
    from motia import Motia, http
    from motia.types import ApiRequest, ApiResponse, FlowContext

    motia = Motia()
    base_path = f"motia/items/{uuid.uuid4().hex[:8]}"

    config = {
        "name": "test-path-params-step",
        "description": "Test path params",
        "triggers": [http("GET", f"/{base_path}/:id")],
    }

    async def handler(input_data: ApiRequest, context: FlowContext) -> ApiResponse:
        item_id = input_data.path_params.get("id")
        return ApiResponse(
            status=200,
            body={"id": item_id},
        )

    motia.add_step(config, "test_path_params_step.py", handler)

    flush_bridge_queue(bridge)
    time.sleep(0.3)

    async with httpx.AsyncClient() as client:
        response = await client.get(f"{api_url}/{base_path}/test123")

    assert response.status_code == 200
    assert response.json()["id"] == "test123"


@pytest.mark.asyncio
async def test_motia_context_trigger_metadata(bridge, api_url, patch_motia_bridge):
    """Test that FlowContext contains correct trigger metadata."""
    from motia import Motia, http
    from motia.types import ApiRequest, ApiResponse, FlowContext

    motia = Motia()
    endpoint_path = f"motia/meta/{uuid.uuid4().hex[:8]}"
    captured_context = {}

    config = {
        "name": "test-context-step",
        "description": "Test context metadata",
        "triggers": [http("GET", f"/{endpoint_path}")],
    }

    async def handler(input_data: ApiRequest, context: FlowContext) -> ApiResponse:
        captured_context["is_api"] = context.is_api()
        captured_context["is_queue"] = context.is_queue()
        captured_context["trigger_type"] = context.trigger.type
        return ApiResponse(
            status=200,
            body={"ok": True},
        )

    motia.add_step(config, "test_context_step.py", handler)

    flush_bridge_queue(bridge)
    time.sleep(0.3)

    async with httpx.AsyncClient() as client:
        response = await client.get(f"{api_url}/{endpoint_path}")

    assert response.status_code == 200
    assert captured_context["is_api"] is True
    assert captured_context["is_queue"] is False
    assert captured_context["trigger_type"] == "http"


def test_motia_step_config_validation(bridge, api_url, patch_motia_bridge):
    """Test that invalid step configs are rejected."""
    from motia import Motia, http

    motia = Motia()

    # Missing required fields should raise an error
    invalid_config = {
        "description": "Missing name",
        "triggers": [http("GET", "/invalid")],
    }

    with pytest.raises((ValueError, Exception)):
        motia.add_step(invalid_config, "invalid_step.py", lambda x, y: None)


def test_motia_add_stream(bridge, api_url, patch_motia_bridge):
    """Test adding a stream to Motia runtime."""
    from motia import Motia
    from motia.types_stream import StreamConfig

    motia = Motia()

    config = StreamConfig(name="test-stream")
    stream = motia.add_stream(config, "test_stream.py")

    assert stream is not None
    assert "test-stream" in motia.streams
    assert motia.streams["test-stream"] is stream


@pytest.mark.asyncio
async def test_motia_multiple_triggers(bridge, api_url, patch_motia_bridge):
    """Test a step with multiple triggers."""
    from motia import Motia, http
    from motia.types import ApiRequest, ApiResponse, FlowContext

    motia = Motia()
    get_path = f"motia/multi/{uuid.uuid4().hex[:8]}"
    post_path = f"motia/multi/{uuid.uuid4().hex[:8]}"

    config = {
        "name": "test-multi-trigger-step",
        "description": "Test step with multiple triggers",
        "triggers": [
            http("GET", f"/{get_path}"),
            http("POST", f"/{post_path}"),
        ],
    }

    async def handler(input_data: ApiRequest, context: FlowContext) -> ApiResponse:
        method = context.trigger.method
        return ApiResponse(
            status=200,
            body={"method": method},
        )

    motia.add_step(config, "test_multi_trigger_step.py", handler)

    flush_bridge_queue(bridge)
    time.sleep(0.3)

    async with httpx.AsyncClient() as client:
        get_response = await client.get(f"{api_url}/{get_path}")
        post_response = await client.post(f"{api_url}/{post_path}", json={})

    assert get_response.status_code == 200
    assert get_response.json()["method"] == "GET"

    assert post_response.status_code == 200
    assert post_response.json()["method"] == "POST"
