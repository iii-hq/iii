"""Owner delivery (sdk.md §1, §4.3): ``obs.emit``, ``obs.call``, ``obs.reject``
and ``FunctionRef.is_observed()`` / ``observers()``, against a fake engine."""

from __future__ import annotations

import asyncio
from typing import Any

import pytest
from observe_fake_engine import (
    FakeEngine,
    acks,
    function_registered,
    observer_added,
    observer_info,
    observer_removed,
    reported_items,
    settle,
    wait_until,
)

from iii import InvocationError, ObserveHooks, Observer
from iii.iii import III


class Owner:
    """A cron-like owner: observers indexed by their ``expression`` arg."""

    def __init__(self, client: III, engine: FakeEngine, function_id: str = "cron::on-tick") -> None:
        self.client = client
        self.engine = engine
        self.function_id = function_id
        self.index: dict[str, list[Observer]] = {}
        self.emitted: list[bool] = []

        def on_tick(tick: dict[str, Any]) -> None:
            for obs in self.index.get(tick["expression"], []):
                self.emitted.append(obs.emit(tick))

        self.ref = client.register_function(
            function_id,
            on_tick,
            observe=ObserveHooks(
                delivery="owner",
                on_observer_added=lambda obs: self.index.setdefault(obs.args["expression"], []).append(obs),
                on_observer_removed=lambda obs: self.index[obs.args["expression"]].remove(obs),
            ),
        )

    def confirm(self, *observers: dict[str, Any], revision: int = 1) -> None:
        self.engine.deliver(function_registered(self.function_id, list(observers), revision=revision))
        self.engine.wait_for(lambda: len(acks(self.engine)) >= len(observers), what="the acks")
        wait_until(lambda: self.client._functions[self.function_id].observe.local_dispatch, what="local dispatch")

    def observer(self, observer_id: str) -> Observer:
        return next(o for o in self.ref.observers() if o.id == observer_id)


def test_emit_returns_false_and_sends_nothing_when_the_filters_reject_the_event(
    engine: FakeEngine, connect: Any
) -> None:
    owner = Owner(connect(engine), engine)
    owner.confirm(observer_info("o", "t", args={"expression": "*/5"}, filters={"$and": [{"input.n": {"$gt": 10}}]}))
    engine.clear()
    owner.client.trigger({"function_id": "cron::on-tick", "payload": {"expression": "*/5", "n": 1}})
    settle(owner.client, engine)
    assert owner.emitted == [False]
    assert engine.frames() == []
    owner.client.trigger({"function_id": "cron::on-tick", "payload": {"expression": "*/5", "n": 11}})
    engine.wait_for(lambda: reported_items(engine), what="the report")
    assert owner.emitted == [False, True]
    [item] = reported_items(engine)
    assert item["matched"] == ["o"]
    assert item["status"] == "success"
    assert item["input"] == {"expression": "*/5", "n": 11}
    assert item["namespace"] == "default"
    assert set(item) >= {"invocation_id", "started_at_ms", "duration_ms", "observe_depth"}


def test_emit_reaches_only_the_observers_the_owner_routes_to(engine: FakeEngine, connect: Any) -> None:
    owner = Owner(connect(engine), engine)
    owner.confirm(
        observer_info("five", "t", args={"expression": "*/5"}),
        observer_info("ten", "t", args={"expression": "*/10"}),
    )
    owner.client.trigger({"function_id": "cron::on-tick", "payload": {"expression": "*/10", "n": 1}})
    owner.client.trigger({"function_id": "cron::on-tick", "payload": {"expression": "* * *", "n": 2}})
    settle(owner.client, engine)
    assert [i["matched"] for i in reported_items(engine)] == [["ten"]]


def test_inside_the_observed_functions_own_handler_the_event_is_that_invocation(
    engine: FakeEngine, connect: Any
) -> None:
    owner = Owner(connect(engine), engine)
    owner.confirm(observer_info("o", "t", args={"expression": "*/5"}, capture=["input", "caller"]))
    seen: list[Any] = []

    def caller_fn(data: Any) -> Any:
        seen.append(owner.client.current_invocation())
        return owner.client.trigger({"function_id": "cron::on-tick", "payload": {"expression": "*/5"}})

    owner.client.register_function("app::caller", caller_fn)
    # an invocation of the owner's handler as the engine routes it
    engine.deliver(
        {
            "type": "invokefunction",
            "invocation_id": "inv-routed",
            "function_id": "cron::on-tick",
            "data": {"expression": "*/5"},
            "caller": {
                "worker_id": "w-9",
                "worker_name": "scheduler",
                "function_id": "sched::fire",
                "invocation_id": "p-1",
            },
            "observe_depth": 2,
        }
    )
    engine.wait_for(lambda: reported_items(engine), what="the report")
    [item] = reported_items(engine)
    assert item["invocation_id"] == "inv-routed"
    assert item["caller"] == {
        "worker_id": "w-9",
        "worker_name": "scheduler",
        "function_id": "sched::fire",
        "invocation_id": "p-1",
    }
    assert item["observe_depth"] == 2


def test_inside_another_handler_that_handler_is_the_caller(engine: FakeEngine, connect: Any) -> None:
    owner = Owner(connect(engine), engine)
    owner.confirm(observer_info("o", "t", args={"expression": "*/5"}, capture=["input", "caller"]))
    obs = owner.observer("o")
    owner.client.register_function("app::other", lambda data: obs.emit({"expression": "*/5"}))
    engine.deliver(
        {
            "type": "invokefunction",
            "invocation_id": "inv-other",
            "function_id": "app::other",
            "data": {},
            "caller": {"worker_id": "w-1", "worker_name": "billing"},
        }
    )
    engine.wait_for(lambda: reported_items(engine), what="the report")
    [item] = reported_items(engine)
    assert item["caller"] == {
        "worker_id": "worker-1",
        "worker_name": "observe-test",
        "function_id": "app::other",
        "invocation_id": "inv-other",
        "parent": {"worker_id": "w-1", "worker_name": "billing"},
    }
    assert item["invocation_id"] != "inv-other"


def test_outside_any_handler_the_caller_is_this_worker_only(engine: FakeEngine, connect: Any) -> None:
    owner = Owner(connect(engine), engine)
    owner.confirm(observer_info("o", "t", args={"expression": "*/5"}, capture=["input", "caller"]))
    assert owner.observer("o").emit({"expression": "*/5"}) is True
    engine.wait_for(lambda: reported_items(engine), what="the report")
    assert reported_items(engine)[0]["caller"] == {"worker_id": "worker-1", "worker_name": "observe-test"}


def test_emits_of_one_event_to_n_observers_in_one_flush_are_one_report_item(engine: FakeEngine, connect: Any) -> None:
    owner = Owner(connect(engine), engine)
    owner.confirm(
        observer_info("a", "t", args={"expression": "*/5"}, capture=["input"]),
        observer_info("b", "t", args={"expression": "*/5"}, capture=["input", "caller"]),
    )
    owner.client.trigger({"function_id": "cron::on-tick", "payload": {"expression": "*/5", "n": 7}})
    settle(owner.client, engine)
    [item] = reported_items(engine)
    assert item["matched"] == ["a", "b"]
    assert "caller" in item  # the union of the captures


def test_not_owner_delivered_on_an_auto_function_and_false_once_removed(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    got: list[Observer] = []
    client.register_function("auto::fn", lambda d: d, observe=ObserveHooks(on_observer_added=got.append))
    engine.deliver(function_registered("auto::fn", [observer_info("o", "t")]))
    engine.wait_for(lambda: acks(engine), what="the ack")
    with pytest.raises(InvocationError) as raised:
        got[0].emit({})
    assert raised.value.code == "NOT_OWNER_DELIVERED"
    with pytest.raises(InvocationError) as raised:
        got[0].call({})
    assert raised.value.code == "NOT_OWNER_DELIVERED"

    owner = Owner(client, engine)
    engine.clear()
    owner.confirm(observer_info("x", "t", args={"expression": "*/5"}))
    obs = owner.observer("x")
    engine.deliver(observer_removed("cron::on-tick", 2, "x"))
    wait_until(lambda: not owner.ref.is_observed(), what="the removal")
    # an owner's tick loop must not abort because one observer went away
    assert obs.emit({"expression": "*/5"}) is False
    with pytest.raises(InvocationError) as raised:
        obs.call({})
    assert raised.value.code == "OBSERVER_NOT_ACTIVE"


def test_reject_sends_a_late_rejection_with_the_announced_revision_and_stops_delivering(
    engine: FakeEngine, connect: Any
) -> None:
    owner = Owner(connect(engine), engine)
    owner.confirm(observer_info("o", "t", args={"expression": "*/5"}), revision=4)
    obs = owner.observer("o")
    engine.clear()
    obs.reject("UNKNOWN_DB", "the database was removed by a config reload")
    engine.wait_for(lambda: acks(engine), what="the late rejection")
    assert acks(engine)[0] == {
        "type": "observerack",
        "function_id": "cron::on-tick",
        "namespace": "default",
        "id": "o",
        "revision": 4,
        "error": {"code": "UNKNOWN_DB", "message": "the database was removed by a config reload"},
    }
    assert owner.ref.is_observed() is False
    assert obs.emit({"expression": "*/5"}) is False

    # a later functionregistered still listing it: re-acked with the rejection, not reactivated
    engine.clear()
    engine.deliver(
        function_registered("cron::on-tick", [observer_info("o", "t", args={"expression": "*/5"})], revision=9)
    )
    engine.wait_for(lambda: acks(engine), what="the re-ack")
    assert acks(engine)[0]["revision"] == 9
    assert acks(engine)[0]["error"]["code"] == "UNKNOWN_DB"
    assert owner.ref.is_observed() is False

    # a fresh observeradded starts it over
    engine.clear()
    engine.deliver(observer_added("cron::on-tick", 10, observer_info("o", "t", args={"expression": "*/5"})))
    engine.wait_for(lambda: acks(engine), what="the fresh ack")
    assert "error" not in acks(engine)[0]
    assert owner.ref.is_observed() is True


def _answer_activations(
    engine: FakeEngine, result: Any = None, error: dict[str, str] | None = None
) -> list[dict[str, Any]]:
    calls: list[dict[str, Any]] = []

    def on_frame(frame: dict[str, Any]) -> list[dict[str, Any]] | None:
        if frame.get("type") != "invokefunction" or "activation" not in frame:
            return None
        calls.append(frame)
        reply: dict[str, Any] = {
            "type": "invocationresult",
            "invocation_id": frame["invocation_id"],
            "function_id": frame["function_id"],
        }
        if error is not None:
            reply["error"] = error
        else:
            reply["result"] = result
        return [reply]

    engine.on_frame = on_frame
    return calls


def _router(client: III, engine: FakeEngine, filters: dict[str, Any] | None = None) -> tuple[Any, dict[str, Observer]]:
    routes: dict[str, Observer] = {}

    async def on_request(req: dict[str, Any]) -> Any:
        obs = routes.get(req["path"])
        return await obs.call_async(req) if obs else {"status": 404}

    ref = client.register_function(
        "http::on-request",
        on_request,
        observe=ObserveHooks(
            delivery="owner",
            on_observer_added=lambda obs: routes.__setitem__(obs.args["path"], obs),
            on_observer_removed=lambda obs: routes.pop(obs.args["path"], None),
        ),
    )
    engine.deliver(
        function_registered(
            "http::on-request",
            [observer_info("route-1", "users::get", args={"path": "/users"}, filters=filters or {})],
        )
    )
    engine.wait_for(lambda: acks(engine), what="the ack")
    wait_until(lambda: client._functions["http::on-request"].observe.local_dispatch, what="local dispatch")
    return ref, routes


def test_call_invokes_the_target_through_the_engine_and_returns_its_result(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    calls = _answer_activations(engine, result={"status": 200})
    _, routes = _router(client, engine)
    assert client.trigger({"function_id": "http::on-request", "payload": {"path": "/users"}}) == {"status": 200}
    [frame] = calls
    assert frame["function_id"] == "users::get"
    assert frame["activation"] == {"observer_id": "route-1"}
    assert frame["data"] == {"path": "/users"}
    assert "namespace" not in frame  # CONTRACT D13
    assert frame["caller"]["function_id"] == "http::on-request"
    assert "observe_depth" not in frame  # 0 is omitted

    # sync and async forms from outside any handler
    assert routes["/users"].call({"path": "/users"}) == {"status": 200}

    async def from_another_loop() -> Any:
        return await routes["/users"].call_async({"path": "/users"}, timeout_ms=2000)

    assert asyncio.run(from_another_loop()) == {"status": 200}


def test_call_rejects_with_filters_not_matched_and_sends_nothing(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    calls = _answer_activations(engine, result="ok")
    _, routes = _router(client, engine, filters={"$and": [{"input.tenant": {"$eq": "acme"}}]})
    engine.clear()
    with pytest.raises(InvocationError) as raised:
        routes["/users"].call({"path": "/users", "tenant": "other"})
    assert raised.value.code == "FILTERS_NOT_MATCHED"
    settle(client, engine)
    assert engine.frames() == []
    assert calls == []
    assert routes["/users"].call({"path": "/users", "tenant": "acme"}) == "ok"


def test_call_surfaces_the_engine_error_code_of_a_refused_owner_delivery(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    _answer_activations(engine, error={"code": "OBSERVER_NOT_ACTIVE", "message": "observer route-1 is pending"})
    _, routes = _router(client, engine)
    with pytest.raises(InvocationError) as raised:
        routes["/users"].call({"path": "/users"})
    assert raised.value.code == "OBSERVER_NOT_ACTIVE"


def test_is_observed_and_observers_reflect_the_active_observers(engine: FakeEngine, connect: Any) -> None:
    owner = Owner(connect(engine), engine)
    assert owner.ref.is_observed() is False
    assert owner.ref.observers() == []
    owner.confirm(observer_info("a", "t", args={"expression": "x"}), observer_info("b", "t", args={"expression": "y"}))
    assert owner.ref.is_observed() is True
    assert [o.id for o in owner.ref.observers()] == ["a", "b"]
    assert owner.ref.observers()[0].to_dict() == {
        "id": "a",
        "function_id": "cron::on-tick",
        "args": {"expression": "x"},
        "filters": {},
        "target": "t",
        "capture": ["input", "output", "caller"],
        "on": "success",
    }
    engine.deliver(observer_removed("cron::on-tick", 2, "a"))
    wait_until(lambda: [o.id for o in owner.ref.observers()] == ["b"], what="the removal")


def test_emit_reports_carry_the_trace_context(engine: FakeEngine, connect: Any) -> None:
    owner = Owner(connect(engine), engine)
    owner.confirm(observer_info("o", "t", args={"expression": "*/5"}))
    traceparent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
    engine.deliver(
        {
            "type": "invokefunction",
            "invocation_id": "inv-traced",
            "function_id": "cron::on-tick",
            "data": {"expression": "*/5"},
            "traceparent": traceparent,
        }
    )
    engine.wait_for(lambda: reported_items(engine), what="the report")
    # the trace the emitting handler runs in: the routed invocation's, or the span the SDK made for it
    assert reported_items(engine)[0]["traceparent"].split("-")[1] == "4bf92f3577b34da6a3ce929d0e0e4736"


def test_a_rejection_made_while_on_observer_added_runs_wins(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)

    def on_added(obs: Observer) -> None:
        obs.reject("ROUTE_CONFLICT", "found a conflict while indexing")

    ref = client.register_function(
        "reject::fn", lambda d: None, observe=ObserveHooks(delivery="owner", on_observer_added=on_added)
    )
    engine.deliver(function_registered("reject::fn", [observer_info("o", "t")], revision=2))
    engine.wait_for(lambda: acks(engine), what="the ack")
    assert acks(engine)[0]["error"] == {"code": "ROUTE_CONFLICT", "message": "found a conflict while indexing"}
    assert acks(engine)[0]["revision"] == 2
    assert ref.is_observed() is False
