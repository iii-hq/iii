"""The observe wire contract (``sdk/fixtures/protocol/observe``): every file
decodes into its message type and encodes back to the same JSON value, and
every worker → engine message the SDK builds equals its fixture as a JSON
value given the same inputs. The two engine-only files carry the 0.25
``observe`` object: no registration this SDK builds has one."""

from __future__ import annotations

import copy
import json
from dataclasses import replace
from pathlib import Path
from typing import Any

import pytest
from observe_fake_engine import (
    FakeEngine,
    acks,
    answers,
    function_registered,
    invoke,
    observer_info,
    reported_items,
    settle,
    wait_until,
)
from opentelemetry import trace

from iii import CallAction, InvocationError, Observer, ObserveRejection
from iii.iii import III
from iii.iii_types import (
    FunctionRegisteredMessage,
    InvocationResultMessage,
    InvocationsObservedMessage,
    InvokeFunctionMessage,
    LocalInvocationStatsMessage,
    ObserveMessage,
    ObserverAckMessage,
    ObserverAddedMessage,
    ObserveResultMessage,
    ObserverInfoWire,
    ObserverRemovedMessage,
    ObserverStateChangedMessage,
    RegisterFunctionMessage,
    RegistrationRejectedMessage,
    UnobserveMessage,
    WorkerRegisteredMessage,
)
from iii.observe_wire import decode_activation, decode_caller, decode_observe_depth
from iii.reporter import ReportInit, build_local_invocation_stats, encode_report_item, function_stats

DIR = Path(__file__).resolve().parents[4] / "fixtures" / "protocol" / "observe"
COVERED: set[str] = set()

#: Engine-only: the 0.25 `observe` object, which no v2 SDK type has.
ENGINE_ONLY = {"registerfunction-v1-owner.json", "registerfunction-v1-not-observable.json"}

#: Messages this SDK sends: decoded and encoded with the encoder it sends them with.
SENT: dict[str, Any] = {
    "observe": ObserveMessage,
    "unobserve": UnobserveMessage,
    "observerack": ObserverAckMessage,
    "registerfunction": RegisterFunctionMessage,
    "invokefunction": InvokeFunctionMessage,
    "invocationresult": InvocationResultMessage,
    "localinvocationstats": LocalInvocationStatsMessage,
}
#: Messages this SDK receives: decoded without loss (``null`` and absent kept apart).
RECEIVED: dict[str, Any] = {
    "observeresult": ObserveResultMessage,
    "observerstatechanged": ObserverStateChangedMessage,
    "functionregistered": FunctionRegisteredMessage,
    "observeradded": ObserverAddedMessage,
    "observerremoved": ObserverRemovedMessage,
    "workerregistered": WorkerRegisteredMessage,
    "registrationrejected": RegistrationRejectedMessage,
    "invocationsobserved": InvocationsObservedMessage,
    "invokefunction": InvokeFunctionMessage,
    "invocationresult": InvocationResultMessage,
}

ENGINE_TRACE_ID = "4bf92f3577b34da6a3ce929d0e0e4736"


def fixture(name: str) -> dict[str, Any]:
    COVERED.add(name)
    return json.loads((DIR / name).read_text(encoding="utf-8"))


def files() -> list[str]:
    return sorted(p.name for p in DIR.iterdir() if p.suffix == ".json")


def wire(model: Any) -> dict[str, Any]:
    """What the SDK puts on the wire for a message model (``_to_dict`` + JSON)."""
    return json.loads(json.dumps(III.__new__(III)._to_dict(model)))


def decoded(model: Any) -> dict[str, Any]:
    """Everything a decoder kept: the fields the frame set, ``null`` included where it is a value."""
    return model.model_dump(mode="json", by_alias=True, exclude_unset=True)  # type: ignore[no-any-return]


def same_trace(frame: dict[str, Any], expected: dict[str, Any]) -> dict[str, Any]:
    """The frame with the fixture's ``traceparent`` once it is in the fixture's trace: the span id is this
    process's, and without a tracer provider (the fake engine's tests run without telemetry) spans carry none."""
    if "traceparent" not in expected:
        return frame
    if "traceparent" in frame:
        assert frame["traceparent"].split("-")[1] == expected["traceparent"].split("-")[1]
    else:
        assert isinstance(trace.get_tracer_provider(), trace.ProxyTracerProvider)
    return {**frame, "traceparent": expected["traceparent"]}


# ── every file: decode and encode back ─────────────────────────────────────


@pytest.mark.parametrize("name", [f for f in files() if f not in ENGINE_ONLY])
def test_every_sdk_fixture_decodes_and_encodes_back(name: str) -> None:
    expected = fixture(name)
    kind = expected["type"]
    assert kind in SENT or kind in RECEIVED, f"no message type for {name}"
    if kind in RECEIVED:
        assert decoded(RECEIVED[kind].model_validate(expected)) == expected
    if kind in SENT:
        assert wire(SENT[kind].model_validate(expected)) == expected


def test_report_items_encode_back_through_the_reporter() -> None:
    for name in [f for f in files() if f.startswith("invocationsobserved")]:
        expected = fixture(name)
        for item in expected["items"]:
            wire_item = InvocationsObservedMessage.model_validate({"items": [item]}).items[0]
            parts = {p: getattr(wire_item, p) for p in ("input", "output", "error", "metadata") if p in item}
            if wire_item.caller is not None:
                parts["caller"] = wire_item.caller.model_dump(exclude_none=True)
            captures = list(parts) + (["output"] if item.get("output_is_input") else [])
            if item.get("output_is_input"):
                parts["output"] = parts["input"]
            encoded = encode_report_item(
                ReportInit(
                    function_id=wire_item.function_id,
                    namespace=wire_item.namespace,
                    invocation_id=wire_item.invocation_id,
                    matched=wire_item.matched,
                    status="error" if wire_item.status == "error" else "success",
                    values=parts,
                    started_at_ms=wire_item.started_at_ms,
                    duration_ms=wire_item.duration_ms,
                    observe_depth=wire_item.observe_depth,
                    traceparent=wire_item.traceparent,
                    baggage=wire_item.baggage,
                    output_is_input=bool(wire_item.output_is_input),
                ),
                captures,
            )
            assert json.loads(encoded) == item


@pytest.mark.parametrize("name", sorted(ENGINE_ONLY))
def test_engine_only_fixtures_carry_the_0_25_observe_object_no_registration_here_has(
    engine: FakeEngine, connect: Any, name: str
) -> None:
    expected = fixture(name)
    assert "observe" in expected
    # what this SDK sends for the same function: top-level fields, never `observe`
    assert "observe" not in RegisterFunctionMessage.model_fields
    client = connect(engine)
    client.register_function(
        expected["id"],
        lambda data: data,
        description=expected["description"],
        request_format=expected["request_format"],
        response_format=expected["request_format"],
        args_format=expected["observe"].get("args_format"),
        private=expected["observe"].get("delivery") == "owner",
        observable=expected["observe"].get("allow", True),
    )
    [frame] = engine.wait_for(lambda: engine.frames_of("registerfunction"), what="registerfunction")
    assert "observe" not in frame


# ── worker → engine: what the SDK builds ───────────────────────────────────


@pytest.mark.parametrize(
    ("name", "source", "target", "options"),
    [
        (
            "observe.json",
            {
                "function_id": "cron::on-tick",
                "namespace": "default",
                "args": {"expression": "0 0 9 * * Mon-Fri"},
                "filters": {"input.scheduled_time": {"$gte": "2026-11-01T00:00:00Z"}},
                "capture": ["input"],
                "on": "success",
                "deliver": "envelope",
            },
            {"function_id": "local::cron-handler", "namespace": "default"},
            {
                "id": "5b0c3e1a-8f2d-4c6b-9a7e-1d2f3a4b5c6d",
                "metadata": {"__binding": "weekday-digest"},
                "max_fires": 1,
                "expires_in_ms": 600000,
                "max_in_flight": 64,
            },
        ),
        # the string shorthand
        (
            "observe-minimal.json",
            "orders::archive",
            "search::reindex-order",
            {"id": "d94f3a5b-6c7d-4e8f-a01b-2c3d4e5f6a7b"},
        ),
        (
            "observe-on-error.json",
            {
                "function_id": "payments::charge",
                "filters": {"input.amount": {"$gte": 500}},
                "capture": ["input", "error", "caller"],
                "on": "error",
            },
            "alerts::payment-failed",
            {"id": "c81e2f4a-5b6c-4d7e-9f0a-1b2c3d4e5f60", "metadata": {"team": "payments-oncall"}, "max_in_flight": 4},
        ),
    ],
)
def test_observe_frames_equal_the_fixture(
    engine: FakeEngine, connect: Any, name: str, source: Any, target: Any, options: Any
) -> None:
    client = connect(engine)
    client.observe(source, target, options)
    frames = engine.wait_for(lambda: engine.frames_of("observe"), what="the observe frame")
    assert frames == [fixture(name)]


def test_unobserve_json_from_dispose(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    handle = client.observe("orders::archive", "search::reindex-order", {"id": "5b0c3e1a-8f2d-4c6b-9a7e-1d2f3a4b5c6d"})
    engine.wait_for(lambda: engine.frames_of("observe"), what="the observe frame")
    handle.dispose()
    assert engine.wait_for(lambda: engine.frames_of("unobserve"), what="unobserve") == [fixture("unobserve.json")]


def test_observerack_json_accepts_observeradded_json(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    added: list[Observer] = []
    client.on_observer_added("cron::on-tick", added.append)
    client.register_function("cron::on-tick", lambda tick: tick, private=True)
    engine.deliver(function_registered("cron::on-tick", revision=12))
    engine.deliver(fixture("observeradded.json"))
    assert engine.wait_for(lambda: acks(engine), what="the ack") == [fixture("observerack.json")]
    [obs] = added
    assert obs.to_dict() == {"function_id": "cron::on-tick", **fixture("observeradded.json")["observer"]}


def test_observerack_error_json_is_the_rejection_on_observer_added_raised(engine: FakeEngine, connect: Any) -> None:
    expected = fixture("observerack-error.json")
    client = connect(engine)

    @client.on_observer_added("cron::on-tick")
    def start(obs: Observer) -> None:
        raise ObserveRejection(expected["error"]["code"], expected["error"]["message"])

    client.register_function("cron::on-tick", lambda tick: tick, private=True)
    engine.deliver(function_registered("cron::on-tick", revision=14))
    added = copy.deepcopy(fixture("observeradded.json"))
    added["revision"] = expected["revision"]
    added["observer"]["id"] = expected["id"]
    added["observer"]["args"] = {"expression": "0 0 9 * * Mon-Fry"}
    engine.deliver(added)
    assert engine.wait_for(lambda: acks(engine), what="the rejection") == [expected]


@pytest.mark.parametrize(
    ("name", "options"),
    [
        ("registerfunction.json", {}),
        ("registerfunction-private.json", {"private": True}),
        ("registerfunction-not-observable.json", {"observable": False}),
    ],
)
def test_registerfunction_frames_equal_the_fixture_and_hooks_and_standby_never_travel(
    engine: FakeEngine, connect: Any, name: str, options: dict[str, Any]
) -> None:
    expected = fixture(name)
    client = connect(engine)

    def handler(data):  # type: ignore[no-untyped-def]  # untyped: no formats extracted
        return data

    client.on_observer_added(expected["id"], lambda obs: None)
    client.on_observer_removed(expected["id"], lambda obs: None)
    client.register_function(
        expected["id"],
        handler,
        description=expected["description"],
        request_format=expected.get("request_format"),
        response_format=expected.get("response_format"),
        args_format=expected.get("args_format"),
        standby=True,
        local_dispatch=False,
        **options,
    )
    assert engine.wait_for(lambda: engine.frames_of("registerfunction"), what="registerfunction") == [expected]


def test_invocationsobserved_json_report_items() -> None:
    expected = fixture("invocationsobserved.json")
    failed, null_output, with_metadata = expected["items"]

    def item(source: dict[str, Any], captures: list[str], values: dict[str, Any]) -> str:
        return encode_report_item(
            ReportInit(
                function_id=source["function_id"],
                namespace="default",
                invocation_id=source["invocation_id"],
                matched=source["matched"],
                status=source["status"],
                values=values,
                started_at_ms=source["started_at_ms"],
                duration_ms=source["duration_ms"],
                observe_depth=0,
                traceparent=source.get("traceparent"),
            ),
            captures,
        )

    items = [
        item(failed, ["input", "error", "caller"], {k: failed[k] for k in ("input", "error", "caller")}),
        # a function that returned None: `output: null` is a value, not absent; called outside any handler
        item(null_output, ["input", "output", "caller"], {"input": null_output["input"], "output": None}),
        item(
            with_metadata,
            ["input", "output", "caller", "metadata"],
            {k: with_metadata[k] for k in ("input", "output", "caller", "metadata")},
        ),
    ]
    frame = json.loads('{"type":"invocationsobserved","items":[' + ",".join(items) + "]}")
    assert frame == expected


def test_invocationsobserved_event_source_json_a_private_source_called_in_a_routed_call(
    engine: FakeEngine, connect: Any
) -> None:
    expected = fixture("invocationsobserved-event-source.json")
    [item] = expected["items"]
    routed = fixture("invokefunction-caller.json")
    client = connect(engine)
    change = item["input"]

    async def execute(data: Any) -> dict[str, Any]:
        await client.call_async({"function_id": "database::on-row-changed", "payload": change})
        return {"affected_rows": 1}

    client.register_function("database::execute", execute)
    client.register_function("database::on-row-changed", lambda event: event, private=True)
    engine.deliver(
        function_registered(
            "database::on-row-changed",
            [observer_info(o, "t", capture=["input", "output", "caller"]) for o in item["matched"]],
        )
    )
    engine.deliver(function_registered("database::execute"))
    engine.wait_for(lambda: len(acks(engine)) == 2, what="the acks")
    wait_until(lambda: client._functions["database::on-row-changed"].observe.local_dispatch, what="local dispatch")
    engine.deliver(routed)
    engine.wait_for(lambda: answers(engine, routed["invocation_id"]), what="the answer")
    settle(client, engine)
    [sent] = reported_items(engine)
    # this call's own id and timings
    assert sent.pop("invocation_id") != routed["invocation_id"]
    assert isinstance(sent.pop("started_at_ms"), int) and isinstance(sent.pop("duration_ms"), (int, float))
    sent.pop("traceparent", None)
    sent.pop("baggage", None)
    expected_item = {
        k: v
        for k, v in item.items()
        if k not in ("invocation_id", "started_at_ms", "duration_ms", "traceparent", "baggage")
    }
    assert sent == expected_item  # input once, output_is_input, the caller chain without this worker's identity


def test_localinvocationstats_json() -> None:
    message = build_local_invocation_stats(
        10000,
        {
            "database::on-row-changed": function_stats(48211, 0, 190.2),
            "database::query": function_stats(37, 2, 15.5),
        },
        {"database::on-row-changed": {"BUFFER_FULL": 12}, "database::execute": {"LATE_DELIVERY": 1}},
    )
    assert wire(message) == fixture("localinvocationstats.json")


def test_invokefunction_workers_register_json_features_in_the_payload(engine: FakeEngine, connect: Any) -> None:
    expected = fixture("invokefunction-workers-register.json")
    client = connect(engine, worker_name="billing", namespace="default")
    [frame] = [f for f in engine.frames_of("invokefunction") if f["function_id"] == "engine::workers::register"]
    data = frame["data"]
    assert frame["action"] == {"type": "void"}
    assert "invocation_id" not in frame  # a void call
    assert data["features"] == expected["data"]["features"]
    assert data["features"] == ["observe.v1", "observe.v2", "local-dispatch.v1", "large-messages.v1"]
    assert "isolation" not in data  # unset: left out, never null
    assert (data["runtime"], data["name"], data["namespace"]) == ("python", "billing", "default")
    assert set(expected["data"]["telemetry"]) <= set(data["telemetry"])
    assert client.worker_id == "worker-1"


# ── routed calls: what the host answers and reports ────────────────────────


def _confirm(client: III, engine: FakeEngine, function_id: str, observers: list[dict[str, Any]]) -> None:
    engine.deliver(function_registered(function_id, observers))
    engine.wait_for(
        lambda: {o["id"] for o in observers} <= {a["id"] for a in acks(engine)}, what=f"the acks of {function_id}"
    )


def test_invocationresult_observed_json_routed_call_matches_ride_the_answer(engine: FakeEngine, connect: Any) -> None:
    expected = fixture("invocationresult-observed.json")
    client = connect(engine)
    client.register_function("orders::create", lambda order: expected["result"])
    _confirm(client, engine, "orders::create", [observer_info(o, "t") for o in expected["observed"]])
    engine.deliver(
        invoke(
            "orders::create",
            {"sku": "A-1", "qty": 2},
            expected["invocation_id"],
            traceparent=expected["traceparent"],
            caller={"worker_id": "w-1", "worker_name": "checkout"},
        )
    )
    [answer] = engine.wait_for(lambda: answers(engine, expected["invocation_id"]), what="the answer")
    assert same_trace(answer, expected) == expected
    settle(client, engine)
    assert reported_items(engine) == []  # no extra frame


def test_invocationsobserved_routed_and_invocationresult_hook_result_json(engine: FakeEngine, connect: Any) -> None:
    [item] = fixture("invocationsobserved-routed.json")["items"]
    expected_answer = fixture("invocationresult-hook-result.json")
    client = connect(engine)
    client.register_function("orders::submit", lambda order: item["output"])

    @client.on_observer_deliver("orders::submit")
    async def accept(call: Any, observers: Any, next: Any) -> dict[str, Any]:
        order = await next(call, observers)
        return {"order_id": order["order_id"], "status": "accepted"}

    _confirm(client, engine, "orders::submit", [observer_info(o, "t") for o in item["matched"]])
    engine.deliver(
        invoke(
            "orders::submit",
            item["input"],
            item["invocation_id"],
            traceparent=item["traceparent"],
            caller={"worker_id": "2b8d4f6a-1c3e-4a5b-9d7f-0e2c4a6b8d1f", "worker_name": "checkout", **item["caller"]},
        )
    )
    [answer] = engine.wait_for(lambda: answers(engine, item["invocation_id"]), what="the answer")
    assert same_trace(answer, expected_answer) == expected_answer  # the hook's value, no `observed`
    frames = engine.frames()
    report_at = next(i for i, f in enumerate(frames) if f["type"] == "invocationsobserved")
    assert report_at < frames.index(answer)  # the items go ahead of the answer
    [sent] = frames[report_at]["items"]
    sent["started_at_ms"], sent["duration_ms"] = item["started_at_ms"], item["duration_ms"]
    assert same_trace(sent, item) == item


def test_invocationsobserved_routed_items_json_next_with_changed_calls(engine: FakeEngine, connect: Any) -> None:
    expected = fixture("invocationsobserved-routed-items.json")
    first, second = expected["items"]
    invocation_id = first["invocation_id"]
    client = connect(engine)
    batch = {"orders": [first["input"], second["input"]]}
    client.register_function("orders::import", lambda data: first["output"])

    @client.on_observer_deliver("orders::import")
    def per_order(call: Any, observers: Any, next: Any) -> Any:
        output = next(call, observers)
        for order in call.input["orders"]:
            next(replace(call, input=order), observers)
        return output

    by_warehouse = [
        observer_info(
            item["matched"][0], "t", filters={"$and": [{"input.warehouse": {"$eq": item["input"]["warehouse"]}}]}
        )
        for item in (first, second)
    ]
    _confirm(client, engine, "orders::import", [*by_warehouse, observer_info("batch-observer", "t")])
    engine.deliver(
        invoke(
            "orders::import",
            batch,
            invocation_id,
            traceparent=first["traceparent"],
            caller={"worker_id": "w-erp", "worker_name": "erp", **first["caller"]},
        )
    )
    [answer] = engine.wait_for(lambda: answers(engine, invocation_id), what="the answer")
    assert answer["observed"] == ["batch-observer"]  # the unchanged call's match rides the answer
    assert answer["result"] == first["output"]
    [report] = engine.frames_of("invocationsobserved")
    assert engine.frames().index(report) < engine.frames().index(answer)
    for sent, item in zip(report["items"], expected["items"]):
        sent["started_at_ms"], sent["duration_ms"] = item["started_at_ms"], item["duration_ms"]
        assert same_trace(sent, item) == item


def test_invocationresult_observed_error_json_shape_an_error_answer_keeps_observed(
    engine: FakeEngine, connect: Any
) -> None:
    expected = fixture("invocationresult-observed-error.json")
    client = connect(engine)

    def charge(data: Any) -> None:
        raise RuntimeError(expected["error"]["message"])

    client.register_function("payments::charge", charge)
    _confirm(client, engine, "payments::charge", [observer_info(o, "t", on="error") for o in expected["observed"]])
    engine.deliver(invoke("payments::charge", {"amount": 1250}, expected["invocation_id"]))
    [answer] = engine.wait_for(lambda: answers(engine, expected["invocation_id"]), what="the answer")
    assert answer["observed"] == expected["observed"]
    assert answer["error"]["message"] == expected["error"]["message"]
    assert "result" not in answer


# ── calls through an observer ──────────────────────────────────────────────


def _http_host(client: III, engine: FakeEngine, target_result: dict[str, Any]) -> Observer:
    """``http::on-request`` routing a request to the observer of route ``/users/:id``; the fake engine answers the
    call through it with ``target_result``."""
    forwarded = fixture("invokefunction-observer-call.json")
    observer_id = forwarded["activation"]["observer_id"]

    def answer(frame: dict[str, Any]) -> list[dict[str, Any]] | None:
        if frame.get("type") != "invokefunction" or frame.get("activation", {}).get("observer_id") != observer_id:
            return None
        if "invocation_id" not in frame:
            return None
        return [{**target_result, "invocation_id": frame["invocation_id"], "function_id": frame["function_id"]}]

    engine.on_frame = answer
    client.register_function("http::on-request", lambda request: request, private=True)

    @client.on_observer_deliver("http::on-request")
    async def route(call: Any, observers: Any, next: Any) -> Any:
        [obs] = [o for o in observers if o.args["path"] == "/users/:id"]
        return await client.call_async({"observer": obs, "payload": forwarded["data"]})

    _confirm(
        client,
        engine,
        "http::on-request",
        [observer_info(observer_id, "users::get", args={"method": "GET", "path": "/users/:id"})],
    )
    return client.observers("http::on-request")[0]


def test_invokefunction_observer_call_json_and_invocationresult_json(engine: FakeEngine, connect: Any) -> None:
    expected = fixture("invokefunction-observer-call.json")
    result = fixture("invocationresult.json")
    client = connect(engine)
    _http_host(client, engine, {"type": "invocationresult", "result": result["result"]})
    caller = expected["caller"]
    # the request the http worker's server got, as a call with no caller of its own
    engine.deliver(
        invoke(
            "http::on-request",
            {"method": "GET", "path": "/users/42"},
            caller["invocation_id"],
            traceparent=expected["traceparent"],
        )
    )
    [answer] = engine.wait_for(lambda: answers(engine, caller["invocation_id"]), what="the answer")
    assert answer["result"] == result["result"]  # the target's result is what the caller gets
    [call] = [f for f in engine.frames_of("invokefunction") if f.get("activation")]
    assert same_trace({**call, "invocation_id": expected["invocation_id"]}, expected) == expected


def test_invokefunction_observer_call_void_json_initial_state_once_accepted(engine: FakeEngine, connect: Any) -> None:
    expected = fixture("invokefunction-observer-call-void.json")
    client = connect(engine)
    client.register_function("inventory::on-stock-changed", lambda event: event, private=True)
    _confirm(
        client,
        engine,
        "inventory::on-stock-changed",
        [observer_info(expected["activation"]["observer_id"], expected["function_id"], args={"sku": "A-1"})],
    )
    [obs] = client.observers("inventory::on-stock-changed")
    obs.accepted(timeout=5)
    assert client.call({"observer": obs, "payload": expected["data"], "action": CallAction.Void()}) is None
    [call] = engine.wait_for(lambda: [f for f in engine.frames_of("invokefunction") if f.get("activation")], what="it")
    assert call == expected


def test_invocationresult_not_function_owner_json_is_the_error_a_refused_call_raises(
    engine: FakeEngine, connect: Any
) -> None:
    refusal = fixture("invocationresult-not-function-owner.json")
    assert fixture("invokefunction-observer-call-namespace.json")["activation"] == {
        "observer_id": refusal["error"]["message"].split()[1]
    }
    client = connect(engine)
    obs = _http_host(client, engine, {"type": "invocationresult", "error": refusal["error"]})
    with pytest.raises(InvocationError) as raised:
        client.call({"observer": obs, "payload": {"path": "/users/7"}})
    assert (raised.value.code, raised.value.message) == (refusal["error"]["code"], refusal["error"]["message"])


# ── engine → worker: what the SDK reads from them ──────────────────────────


@pytest.mark.parametrize(
    "name",
    [
        "invokefunction-observer-call-forwarded.json",
        "invokefunction-legacy.json",
        "invokefunction-activation.json",
        "invokefunction-activation-engine-source.json",
        "invokefunction-caller.json",
    ],
)
def test_inbound_invokefunction_caller_depth_and_activation(name: str) -> None:
    expected = fixture(name)
    assert decode_caller(expected.get("caller")) == expected.get("caller")
    assert decode_observe_depth(expected.get("observe_depth")) == expected.get("observe_depth", 0)
    assert decode_activation(expected.get("activation")) == expected.get("activation")


@pytest.mark.parametrize(
    "name", ["registrationrejected-unsupported.json", "registrationrejected-invalid-args-format.json"]
)
def test_a_function_registration_rejection_keeps_the_connection(engine: FakeEngine, connect: Any, name: str) -> None:
    rejection = fixture(name)
    client = connect(engine)
    client.register_function(rejection["function_id"], lambda d: d)
    engine.deliver(function_registered(rejection["function_id"]))
    wait_until(lambda: client._functions[rejection["function_id"]].observe.confirmed, what="confirmed")
    engine.deliver(rejection)
    wait_until(lambda: not client._functions[rejection["function_id"]].observe.confirmed, what="the rejection")
    assert client.get_connection_state() == "connected"
    assert client._fatal_error is None


@pytest.mark.parametrize(
    ("name", "v2"), [("workerregistered.json", True), ("workerregistered-v1.json", False)]
)
def test_workerregistered_features_decide_whether_this_host_delivers_routed_calls(
    engine_factory: Any, connect: Any, name: str, v2: bool
) -> None:
    features = fixture(name)["features"]
    engine = engine_factory(features)
    client = connect(engine)
    assert client._delivers_v2() is v2


def test_workerregistered_legacy_json_observe_fails_unsupported(engine_factory: Any, connect: Any) -> None:
    assert "features" not in fixture("workerregistered-legacy.json")
    client = connect(engine_factory("legacy"))
    handle = client.observe("orders::create", "t::x")
    with pytest.raises(ObserveRejection) as raised:
        handle.ready(timeout=5)
    assert raised.value.code == "UNSUPPORTED"


def test_accepts_null_for_optional_fields_and_keeps_unknown_enum_values() -> None:
    result = ObserveResultMessage.model_validate({"id": "a", "state": "hibernating", "error": None, "warnings": None})
    assert (result.id, result.state, result.error, result.warnings) == ("a", "hibernating", None, None)
    changed = ObserverStateChangedMessage.model_validate({"id": "a", "state": "active", "reason": None, "fires": None})
    assert (changed.reason, changed.fires) == (None, 0)
    worker = WorkerRegisteredMessage.model_validate({"worker_id": "w", "reattach_token": None, "features": None})
    assert (worker.reattach_token, worker.features) == (None, None)
    info = ObserverInfoWire.model_validate(
        {"id": "o", "args": None, "filters": None, "target": "t", "capture": ["input", "telemetry"], "on": "start"}
    )
    assert (info.args, info.filters, info.capture, info.on) == ({}, {}, ["input", "telemetry"], "start")
    defaults = ObserverInfoWire.model_validate({"id": "o", "target": "t"})
    assert (defaults.capture, defaults.on) == (["input", "output", "caller"], "success")
    registered = FunctionRegisteredMessage.model_validate(
        {"id": "f", "namespace": "default", "revision": None, "observers": None}
    )
    assert (registered.revision, registered.observers, registered.local_dispatch) == (0, [], False)
    answer = InvocationResultMessage.model_validate(
        {"invocation_id": "i", "function_id": "f", "result": 1, "observed": None}
    )
    assert answer.observed is None and "observed" not in wire(answer)
    explicit_defaults = RegisterFunctionMessage.model_validate({"id": "f", "private": False, "observable": True})
    assert (explicit_defaults.private, explicit_defaults.observable) == (False, True)
    item = InvocationsObservedMessage.model_validate(
        {"items": [{"function_id": "f", "invocation_id": "i", "status": "success", "output_is_input": False}]}
    ).items[0]
    assert item.output_is_input is False
    caller = decode_caller(
        {"worker_id": "w", "worker_name": 7, "parent": {"worker_id": "p", "parent": {"worker_id": "gp"}}}
    )
    assert caller == {"worker_id": "w", "parent": {"worker_id": "p"}}
    assert decode_observe_depth(None) == 0 and decode_observe_depth("2") == 0 and decode_observe_depth(True) == 0
    assert decode_activation(None) is None and decode_activation({"activation_id": "x"}) is None


def test_every_fixture_file_is_exercised() -> None:
    # runs last in this module (pytest keeps file order)
    assert [f for f in files() if f not in COVERED] == []
