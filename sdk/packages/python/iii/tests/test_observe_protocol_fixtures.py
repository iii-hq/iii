"""The observe wire contract (``sdk/fixtures/protocol/observe``, CONTRACT D5):
every worker → engine message the SDK builds equals its fixture as a JSON value
given the same inputs, and every engine → worker message decodes without loss."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

import pytest
from observe_fake_engine import FakeEngine, acks, function_registered

from iii import ObserveHooks, ObserveRejection
from iii.iii import III
from iii.iii_types import (
    FunctionRegisteredMessage,
    InvokeFunctionMessage,
    LocalInvocationStatsMessage,
    ObserveMessage,
    ObserverAckMessage,
    ObserverAddedMessage,
    ObserveResultMessage,
    ObserverInfoWire,
    ObserverRemovedMessage,
    ObserverStateChangedMessage,
    WorkerRegisteredMessage,
)
from iii.invocation_context import ActiveInvocation
from iii.observe_wire import caller_for, decode_activation, decode_caller, decode_observe_depth
from iii.reporter import ReportInit, build_local_invocation_stats, encode_report_item, function_stats

DIR = Path(__file__).resolve().parents[4] / "fixtures" / "protocol" / "observe"
COVERED: set[str] = set()


def fixture(name: str) -> dict[str, Any]:
    COVERED.add(name)
    return json.loads((DIR / name).read_text(encoding="utf-8"))


def wire(model: Any) -> dict[str, Any]:
    """What the SDK puts on the wire for a message model (``_to_dict`` + JSON)."""
    return json.loads(json.dumps(III.__new__(III)._to_dict(model)))


# ── worker → engine ────────────────────────────────────────────────────────


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
        (
            "observe-minimal.json",
            {"function_id": "orders::archive"},
            "search::reindex-order",
            {"id": "d94f3a5b-6c7d-4e8f-a01b-2c3d4e5f6a7b"},
        ),
        (
            "observe-auto.json",
            {
                "function_id": "payments::charge",
                "filters": {"input.amount": {"$gte": 500}},
                "capture": ["input", "error", "caller"],
                "on": "error",
            },
            "alerts::payment-failed",
            {"id": "c81e2f4a-5b6c-4d7e-9f0a-1b2c3d4e5f60", "max_in_flight": 4},
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
    handle = client.observe(
        {"function_id": "orders::archive"}, "search::reindex-order", {"id": "5b0c3e1a-8f2d-4c6b-9a7e-1d2f3a4b5c6d"}
    )
    engine.wait_for(lambda: engine.frames_of("observe"), what="the observe frame")
    handle.dispose()
    assert engine.wait_for(lambda: engine.frames_of("unobserve"), what="unobserve") == [fixture("unobserve.json")]


@pytest.mark.parametrize("name", ["observerack.json", "observerack-error.json"])
def test_observerack_json_the_owner_acks_observeradded_json(engine: FakeEngine, connect: Any, name: str) -> None:
    def on_added(obs: Any) -> None:
        if name == "observerack-error.json":
            raise ObserveRejection("INVALID_CRON_EXPRESSION", "invalid day-of-week field")

    client = connect(engine)
    client.register_function(
        "cron::on-tick", lambda d: None, observe=ObserveHooks(delivery="owner", on_observer_added=on_added)
    )
    engine.deliver(function_registered("cron::on-tick", revision=12))
    engine.deliver(fixture("observeradded.json"))
    assert engine.wait_for(lambda: acks(engine), what="the ack") == [fixture(name)]


def test_invocationsobserved_json_report_items() -> None:
    expected = fixture("invocationsobserved.json")
    failed, null_output, emitted = expected["items"]
    items = [
        encode_report_item(
            ReportInit(
                function_id="payments::charge",
                namespace="default",
                invocation_id=failed["invocation_id"],
                matched=failed["matched"],
                status="error",
                values={k: failed[k] for k in ("input", "error", "caller", "metadata")},
                started_at_ms=1760350000123,
                duration_ms=12.75,
                observe_depth=0,
                traceparent=failed["traceparent"],
            ),
            ["input", "error", "caller", "metadata"],
        ),
        encode_report_item(
            ReportInit(
                function_id="orders::archive",
                namespace="default",
                invocation_id=null_output["invocation_id"],
                matched=null_output["matched"],
                status="success",
                # a function that returned None: `output: null` is a value, not absent
                values={"input": null_output["input"], "output": None, "caller": null_output["caller"]},
                started_at_ms=1760350000180,
                duration_ms=0.42,
                observe_depth=1,
            ),
            ["input", "output", "caller", "metadata"],
        ),
        encode_report_item(
            ReportInit(
                function_id="database::on-row-changed",
                namespace="default",
                invocation_id=emitted["invocation_id"],
                matched=emitted["matched"],
                status="success",
                values={"input": emitted["input"], "caller": emitted["caller"]},
                started_at_ms=1760350000201,
                duration_ms=0.004,
                observe_depth=0,
                traceparent=emitted["traceparent"],
                baggage=emitted["baggage"],
            ),
            ["input", "caller"],
        ),
    ]
    frame = json.loads('{"type":"invocationsobserved","items":[' + ",".join(items) + "]}")
    assert frame == expected


def test_localinvocationstats_json() -> None:
    message = build_local_invocation_stats(
        10000,
        {
            "database::on-row-changed": function_stats(48211, 0, 190.2),
            "orders::archive": function_stats(37, 2, 15.5),
        },
        {"database::on-row-changed": {"BUFFER_FULL": 12}},
    )
    assert wire(message) == fixture("localinvocationstats.json")
    assert wire(LocalInvocationStatsMessage.model_validate(fixture("localinvocationstats.json"))) == fixture(
        "localinvocationstats.json"
    )


@pytest.mark.parametrize(
    ("name", "observe", "local_dispatch"),
    [
        ("registerfunction.json", None, True),
        (
            "registerfunction-owner.json",
            ObserveHooks(
                delivery="owner",
                args_format={
                    "type": "object",
                    "additionalProperties": False,
                    "required": ["expression"],
                    "properties": {"expression": {"type": "string"}},
                },
                legacy_trigger_type="cron",
                allow=True,
                on_observer_added=lambda obs: None,
                on_observer_removed=lambda obs: None,
            ),
            False,
        ),
        ("registerfunction-not-observable.json", {"delivery": "auto", "allow": False}, True),
    ],
)
def test_registerfunction_hooks_and_local_dispatch_never_travel(
    engine: FakeEngine, connect: Any, name: str, observe: Any, local_dispatch: bool
) -> None:
    expected = fixture(name)
    client = connect(engine)

    def handler(data):  # type: ignore[no-untyped-def]  # untyped: no formats extracted
        return None

    client.register_function(
        expected["id"],
        handler,
        description=expected["description"],
        request_format=expected.get("request_format"),
        response_format=expected.get("response_format"),
        observe=observe,
        local_dispatch=local_dispatch,
    )
    assert engine.wait_for(lambda: engine.frames_of("registerfunction"), what="registerfunction") == [expected]


def test_invokefunction_owner_call_json_activation_caller_no_namespace_depth_0_omitted() -> None:
    expected = fixture("invokefunction-owner-call.json")
    caller = caller_for(
        ActiveInvocation(function_id="http::on-request", invocation_id="9b1d3f5a-7c9e-4b1d-8f3a-5c7e9b1d3f5a")
    )
    built = InvokeFunctionMessage(
        invocation_id=expected["invocation_id"],
        function_id="users::get",
        data=expected["data"],
        traceparent=expected["traceparent"],
        caller=caller,
        observe_depth=None,  # 0: omitted
        activation={"observer_id": "4f2a9b1c-7d3e-4f6a-8b5c-2e1d0f9a8b7c"},
    )
    assert wire(built) == expected


@pytest.mark.parametrize(
    "name",
    [
        "invokefunction-owner-call-namespace.json",
        "invokefunction-legacy.json",
        "invokefunction-activation.json",
        "invokefunction-caller.json",
    ],
)
def test_invokefunction_fixtures_decode_and_reencode(name: str) -> None:
    expected = fixture(name)
    decoded = InvokeFunctionMessage.model_validate(expected)
    assert wire(decoded) == expected
    # what the SDK's inbound path reads from it
    assert decode_caller(expected.get("caller")) == expected.get("caller")
    assert decode_observe_depth(expected.get("observe_depth")) == expected.get("observe_depth", 0)
    assert decode_activation(expected.get("activation")) == expected.get("activation")
    if name == "invokefunction-legacy.json":
        assert (decoded.caller, decoded.observe_depth, decoded.activation) == (None, None, None)


def test_invokefunction_workers_register_json_features_in_the_payload(engine: FakeEngine, connect: Any) -> None:
    expected = fixture("invokefunction-workers-register.json")
    client = connect(engine, worker_name="billing", namespace="default")
    [frame] = [f for f in engine.frames_of("invokefunction") if f["function_id"] == "engine::workers::register"]
    data = frame["data"]
    assert frame["action"] == {"type": "void"}
    assert "invocation_id" not in frame  # a void call
    assert data["features"] == expected["data"]["features"] == ["observe.v1", "local-dispatch.v1"]
    # every field of the contract is there; runtime, version, os, pid and telemetry are this host's
    assert set(expected["data"]) <= set(data)
    assert (data["runtime"], data["name"], data["namespace"]) == ("python", "billing", "default")
    assert set(expected["data"]["telemetry"]) <= set(data["telemetry"])
    assert client.worker_id == "worker-1"


def test_unregistertriggertype_json(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    with pytest.warns(DeprecationWarning):
        client.unregister_trigger_type({"id": "webhook", "description": "Webhooks"})
    frames = engine.wait_for(lambda: engine.frames_of("unregistertriggertype"), what="the frame")
    assert frames == [fixture("unregistertriggertype.json")]


# ── engine → worker ────────────────────────────────────────────────────────

DECODERS: dict[str, Any] = {
    "workerregistered.json": WorkerRegisteredMessage,
    "workerregistered-legacy.json": WorkerRegisteredMessage,
    "observeresult.json": ObserveResultMessage,
    "observeresult-error.json": ObserveResultMessage,
    "observerstatechanged.json": ObserverStateChangedMessage,
    "observerstatechanged-reason.json": ObserverStateChangedMessage,
    "functionregistered.json": FunctionRegisteredMessage,
    "functionregistered-empty.json": FunctionRegisteredMessage,
    "observeradded.json": ObserverAddedMessage,
    "observeradded-legacy.json": ObserverAddedMessage,
    "observerremoved.json": ObserverRemovedMessage,
}


@pytest.mark.parametrize("name", sorted(DECODERS))
def test_engine_to_worker_fixtures_decode_without_loss(name: str) -> None:
    expected = fixture(name)
    assert wire(DECODERS[name].model_validate(expected)) == expected


@pytest.mark.parametrize("name", ["observe.json", "observe-minimal.json", "observe-auto.json", "unobserve.json"])
def test_worker_to_engine_fixtures_also_round_trip_through_the_models(name: str) -> None:
    expected = fixture(name)
    model = ObserveMessage if expected["type"] == "observe" else None
    if model is not None:
        assert wire(model.model_validate(expected)) == expected
    acked = fixture("observerack-error.json")
    assert wire(ObserverAckMessage.model_validate(acked)) == acked


def test_accepts_null_for_optional_fields_and_keeps_unknown_enum_values() -> None:
    result = ObserveResultMessage.model_validate({"id": "a", "state": "hibernating", "error": None, "warnings": None})
    assert (result.id, result.state, result.error, result.warnings) == ("a", "hibernating", None, None)
    changed = ObserverStateChangedMessage.model_validate({"id": "a", "state": "active", "reason": None, "fires": None})
    assert (changed.reason, changed.fires) == (None, 0)
    worker = WorkerRegisteredMessage.model_validate({"worker_id": "w", "reattach_token": None, "features": None})
    assert (worker.reattach_token, worker.features) == (None, None)
    info = ObserverInfoWire.model_validate(
        {
            "id": "o",
            "args": None,
            "filters": None,
            "target": "t",
            "capture": ["input", "telemetry"],
            "on": "start",
            "legacy": None,
        }
    )
    assert (info.args, info.filters, info.capture, info.on, info.legacy) == (
        {},
        {},
        ["input", "telemetry"],
        "start",
        None,
    )
    defaults = ObserverInfoWire.model_validate({"id": "o", "target": "t"})
    assert (defaults.capture, defaults.on) == (["input", "output", "caller"], "success")
    registered = FunctionRegisteredMessage.model_validate(
        {"id": "f", "namespace": "default", "revision": None, "observers": None}
    )
    assert (registered.revision, registered.observers, registered.local_dispatch) == (0, [], False)
    caller = decode_caller(
        {"worker_id": "w", "worker_name": 7, "parent": {"worker_id": "p", "parent": {"worker_id": "gp"}}}
    )
    assert caller == {"worker_id": "w", "parent": {"worker_id": "p"}}
    assert decode_observe_depth(None) == 0 and decode_observe_depth("2") == 0 and decode_observe_depth(True) == 0
    assert decode_activation(None) is None and decode_activation({"activation_id": "x"}) is None


def test_every_fixture_file_is_exercised() -> None:
    # runs last in this module (pytest keeps file order)
    files = sorted(p.name for p in DIR.iterdir() if p.suffix == ".json")
    assert [f for f in files if f not in COVERED] == []
