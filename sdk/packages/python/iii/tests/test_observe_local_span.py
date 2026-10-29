"""Tracing on the local path and on sync handler threads: a local call gets
the ``execute <fn>`` span a remote one gets, its deliver hook runs inside it,
and a sync handler's thread starts in a copy of the context, so the
OpenTelemetry context (and the current invocation) follow it."""

from __future__ import annotations

from collections.abc import Iterator
from typing import Any

import pytest
from observe_fake_engine import FakeEngine, answers, function_registered, invoke, wait_until
from opentelemetry import trace
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import SimpleSpanProcessor
from opentelemetry.sdk.trace.export.in_memory_span_exporter import InMemorySpanExporter


@pytest.fixture
def spans(monkeypatch: pytest.MonkeyPatch) -> Iterator[InMemorySpanExporter]:
    exporter = InMemorySpanExporter()
    provider = TracerProvider()
    provider.add_span_processor(SimpleSpanProcessor(exporter))
    monkeypatch.setattr(trace, "get_tracer", lambda name, *args, **kwargs: provider.get_tracer(name))
    yield exporter
    provider.shutdown()


def test_creates_the_execute_span_for_a_local_call_as_for_a_remote_one(
    engine: FakeEngine, connect: Any, spans: InMemorySpanExporter
) -> None:
    client = connect(engine)
    seen: list[Any] = []

    def handler(data: Any) -> str:
        seen.append(trace.get_current_span().get_span_context())
        return "ok"

    client.register_function("span::fn", handler)
    engine.deliver(function_registered("span::fn"))
    wait_until(lambda: client._functions["span::fn"].observe.local_dispatch, what="local dispatch")
    tracer = trace.get_tracer("test")
    with tracer.start_as_current_span("caller") as parent:
        assert client.call({"function_id": "span::fn", "payload": {}}) == "ok"
    [execute] = [s for s in spans.get_finished_spans() if s.name == "execute span::fn"]
    assert execute.parent is not None and execute.parent.span_id == parent.get_span_context().span_id
    assert execute.kind == trace.SpanKind.INTERNAL
    assert seen[0].span_id == execute.context.span_id  # the handler runs inside it
    events = [e.name for e in execute.events]
    assert events == ["iii.invocation.input", "iii.invocation.output"]


def test_a_sync_handler_of_a_remote_invocation_runs_in_the_inbound_trace_and_invocation(
    engine: FakeEngine, connect: Any, spans: InMemorySpanExporter
) -> None:
    client = connect(engine)
    seen: dict[str, Any] = {}

    def handler(data: Any) -> None:
        seen["span"] = trace.get_current_span().get_span_context()
        seen["inv"] = client.current_invocation()

    client.register_function("span::remote", handler)
    engine.deliver(
        {
            "type": "invokefunction",
            "invocation_id": "inv-1",
            "function_id": "span::remote",
            "data": {},
            "traceparent": "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        }
    )
    engine.wait_for(lambda: engine.frames_of("invocationresult"), what="the answer")
    assert seen["span"].trace_id == 0x4BF92F3577B34DA6A3CE929D0E0E4736  # not lost on the handler's thread
    [execute] = [s for s in spans.get_finished_spans() if s.name == "execute span::remote"]
    assert seen["span"].span_id == execute.context.span_id
    assert seen["inv"].invocation_id == "inv-1"


@pytest.mark.parametrize("routed", [False, True])
def test_the_deliver_hook_runs_inside_the_functions_execute_span(
    engine: FakeEngine, connect: Any, spans: InMemorySpanExporter, routed: bool
) -> None:
    client = connect(engine)
    seen: list[Any] = []

    @client.on_observer_deliver("span::hooked")
    def hook(call: Any, observers: Any, next: Any) -> Any:
        seen.append(trace.get_current_span().get_span_context())
        return next(call, observers)

    client.register_function("span::hooked", lambda data: "ok")
    engine.deliver(function_registered("span::hooked"))
    wait_until(lambda: client._functions["span::hooked"].observe.local_dispatch, what="local dispatch")
    if routed:
        engine.deliver(invoke("span::hooked", {}, "inv-hooked"))
        engine.wait_for(lambda: answers(engine, "inv-hooked"), what="the answer")
    else:
        assert client.call({"function_id": "span::hooked", "payload": {}}) == "ok"
    [execute] = [s for s in spans.get_finished_spans() if s.name == "execute span::hooked"]
    assert seen[0].span_id == execute.context.span_id
