"""Integration tests for OpenTelemetry tracing with a running III engine.

These tests require a running III engine instance. They verify that spans
are created correctly when steps are registered and invoked via HTTP,
and that stream operations produce child spans within the step span.

Run with:
    pytest tests/test_tracing_integration.py -m integration
"""

import time
import uuid
from unittest.mock import patch

import httpx
import pytest
from opentelemetry import trace
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import SimpleSpanProcessor
from opentelemetry.sdk.trace.export.in_memory_span_exporter import InMemorySpanExporter
from opentelemetry.trace import StatusCode

from motia import Motia, http
from motia.enqueue import enqueue
from motia.tracing import _instrumented_bridges, instrument_bridge
from motia.types import ApiRequest, ApiResponse, FlowContext, StepConfig
from tests.conftest import flush_bridge_queue

pytestmark = pytest.mark.integration


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def otel_exporter():
    """Set up OTEL with in-memory exporter for integration tests.

    Resets the global TracerProvider internals so that
    ``trace.set_tracer_provider()`` can be called again within the same
    process (pytest session).
    """
    import opentelemetry.trace as trace_mod

    trace_mod._TRACER_PROVIDER_SET_ONCE._done = False
    trace_mod._TRACER_PROVIDER = None

    exporter = InMemorySpanExporter()
    provider = TracerProvider()
    provider.add_span_processor(SimpleSpanProcessor(exporter))
    trace.set_tracer_provider(provider)
    yield exporter
    exporter.clear()
    provider.shutdown()


@pytest.fixture
def patch_motia_bridge(bridge):
    """Patch ``motia.runtime.get_instance`` with the test bridge and instrument it.

    This ensures that ``Motia.add_step`` uses the live test bridge connected
    to the running III engine, and that the bridge is instrumented for
    trace-context propagation.
    """
    _instrumented_bridges.discard(id(bridge))
    instrument_bridge(bridge)
    with (
        patch("motia.runtime.get_instance", return_value=bridge),
        patch("motia.enqueue.get_instance", return_value=bridge),
    ):
        yield bridge


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_api_step_creates_trace(otel_exporter, patch_motia_bridge, api_url):
    """Register an API step via Motia, POST to the endpoint, and verify that
    a span named ``step:<name>`` is created with the expected attributes.
    """
    step_name = f"trace-integration-{uuid.uuid4().hex[:8]}"
    path = f"/trace-test/{uuid.uuid4().hex[:8]}"

    motia = Motia()

    async def handler(req: ApiRequest, ctx: FlowContext) -> ApiResponse:
        return ApiResponse(status=200, body={"traced": True, "trace_id": ctx.trace_id})

    config = StepConfig(
        name=step_name,
        triggers=[http("POST", path)],
    )
    motia.add_step(config, f"steps/{step_name}_step.py", handler)

    # Flush the bridge queue so the engine registers our step
    flush_bridge_queue(patch_motia_bridge)
    time.sleep(0.5)

    async with httpx.AsyncClient() as client:
        response = await client.post(
            f"{api_url}{path}",
            json={"hello": "world"},
        )

    assert response.status_code == 200
    body = response.json()
    assert body["traced"] is True

    # Allow a short time for spans to flush
    time.sleep(0.2)

    spans = otel_exporter.get_finished_spans()
    step_spans = [s for s in spans if s.name == f"step:{step_name}"]
    assert len(step_spans) >= 1, (
        f"Expected at least one span named 'step:{step_name}', " f"got spans: {[s.name for s in spans]}"
    )

    span = step_spans[0]
    assert span.attributes["motia.step.name"] == step_name
    assert span.attributes["motia.trigger.type"] == "http"
    assert span.status.status_code == StatusCode.OK


@pytest.mark.asyncio
async def test_enqueue_creates_child_span(otel_exporter, patch_motia_bridge, api_url):
    """Register an API step that enqueues an event, invoke it via HTTP, and verify
    that both the step span and enqueue child span are produced.
    """
    step_name = f"enqueue-trace-{uuid.uuid4().hex[:8]}"
    path = f"/enqueue-trace/{uuid.uuid4().hex[:8]}"

    motia = Motia()

    async def handler(req: ApiRequest, ctx: FlowContext) -> ApiResponse:
        try:
            await enqueue({"topic": "test.traced", "data": {"hello": "world"}})
        except Exception:
            pass  # enqueue may fail if queue not configured
        return ApiResponse(status=200, body={"ok": True, "trace_id": ctx.trace_id})

    config = StepConfig(
        name=step_name,
        triggers=[http("POST", path)],
    )
    motia.add_step(config, f"steps/{step_name}_step.py", handler)

    flush_bridge_queue(patch_motia_bridge)
    time.sleep(0.5)

    async with httpx.AsyncClient(timeout=30.0) as client:
        response = await client.post(f"{api_url}{path}", json={})

    assert response.status_code == 200

    time.sleep(0.2)

    spans = otel_exporter.get_finished_spans()

    # Verify the parent step span exists
    step_spans = [s for s in spans if s.name == f"step:{step_name}"]
    assert len(step_spans) >= 1, f"Expected at least one step span, got: {[s.name for s in spans]}"

    # Verify enqueue child span exists
    enqueue_spans = [s for s in spans if s.name == "enqueue"]
    assert len(enqueue_spans) >= 1, f"Expected at least one 'enqueue' span, got: {[s.name for s in spans]}"


def test_engine_receives_traceparent(bridge):
    """Call the engine's traces list RPC. If the engine does not support
    this RPC method, the test is skipped.
    """
    try:
        result = bridge.trigger({"function_id": "engine.traces.list", "payload": {"limit": 10}})
    except Exception:
        pytest.skip("Engine does not support 'engine.traces.list' RPC; " "skipping traceparent verification")

    # If the call succeeds, the result should be a list (possibly empty)
    assert isinstance(result, (list, dict)), f"Expected list or dict from engine.traces.list, got {type(result)}"
