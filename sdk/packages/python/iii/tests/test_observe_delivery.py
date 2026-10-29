"""Delivery in the worker that hosts a function: iii's own (``next``), an
``on_observer_deliver`` hook, keyed delivery, ``is_observed``, calls through
an observer, ``private`` and ``output_is_input``, older engines and
``standby``. Against a fake engine."""

from __future__ import annotations

import asyncio
import logging
import threading
import time
from dataclasses import replace
from typing import Any

import pytest
from observe_fake_engine import (
    V1_FEATURES,
    FakeEngine,
    acks,
    answers,
    function_registered,
    invoke,
    observer_added,
    observer_info,
    observer_removed,
    rejected,
    reported_items,
    settle,
    wait_until,
)

from iii import Call, CallAction, InvocationError, Observer, ObserveRejection, keyed
from iii.iii import III


def _confirm(client: III, engine: FakeEngine, function_id: str, *observers: dict[str, Any]) -> None:
    engine.deliver(function_registered(function_id, list(observers)))
    wait_until(lambda: client._functions[function_id].observe.confirmed, what=f"{function_id} confirmed")
    if observers:
        wanted = {o["id"] for o in observers}
        engine.wait_for(
            lambda: wanted <= {a["id"] for a in acks(engine) if a["function_id"] == function_id}, what="the acks"
        )


def _items(engine: FakeEngine, function_id: str) -> list[dict[str, Any]]:
    return [i for i in reported_items(engine) if i["function_id"] == function_id]


# ── iii's delivery: next ───────────────────────────────────────────────────


def test_iii_delivers_the_observers_whose_on_and_filters_match(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)

    def create(order: dict[str, Any]) -> dict[str, Any]:
        if order["qty"] < 0:
            raise ValueError("negative")
        return {"total": order["qty"] * 10}

    client.register_function("orders::create", create)
    _confirm(
        client,
        engine,
        "orders::create",
        observer_info("big", "t", filters={"$and": [{"output.total": {"$gte": 100}}]}),
        observer_info("errors", "t", on="error", capture=["input", "error"]),
        observer_info("all", "t", on="complete", capture=["input"]),
    )
    client.call({"function_id": "orders::create", "payload": {"qty": 20}})
    client.call({"function_id": "orders::create", "payload": {"qty": 1}})
    with pytest.raises(InvocationError):
        client.call({"function_id": "orders::create", "payload": {"qty": -1}})
    settle(client, engine)
    assert [sorted(i["matched"]) for i in _items(engine, "orders::create")] == [
        ["all", "big"],
        ["all"],
        ["all", "errors"],
    ]


def test_a_deliver_hook_picks_observers_by_their_args_and_next_returns_the_output(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    seen: list[tuple[Any, list[str]]] = []

    @client.on_observer_deliver("cron::on-tick")
    def by_expression(call: Call[Any, Any], observers: Any, next: Any) -> Any:
        seen.append((call.input, [o.id for o in observers]))
        return next(call, [o for o in observers if o.args["expression"] == call.input["expression"]])

    client.register_function("cron::on-tick", lambda tick: tick, private=True)
    _confirm(
        client,
        engine,
        "cron::on-tick",
        observer_info("five", "t", args={"expression": "*/5"}),
        observer_info("ten", "t", args={"expression": "*/10"}),
    )
    tick = {"expression": "*/10", "n": 1}
    assert client.call({"function_id": "cron::on-tick", "payload": tick}) is tick  # private: by reference
    settle(client, engine)
    assert seen == [(tick, ["five", "ten"])]
    assert [i["matched"] for i in _items(engine, "cron::on-tick")] == [["ten"]]


@pytest.mark.parametrize("hook_kind", ["sync", "async"])
def test_what_the_hook_returns_is_what_the_caller_gets(engine: FakeEngine, connect: Any, hook_kind: str) -> None:
    client = connect(engine)

    if hook_kind == "async":

        async def answer(call: Call[Any, Any], observers: Any, next: Any) -> Any:
            output = await next(call, observers)
            return {"wrapped": output}

    else:

        def answer(call: Call[Any, Any], observers: Any, next: Any) -> Any:
            return {"wrapped": next(call, observers)}

    client.on_observer_deliver("wrap::fn", answer)
    client.register_function("wrap::fn", lambda data: data["n"] * 2)
    _confirm(client, engine, "wrap::fn", observer_info("o", "t", capture=["input", "output"]))
    assert client.call({"function_id": "wrap::fn", "payload": {"n": 2}}) == {"wrapped": 4}

    async def from_a_loop() -> Any:
        return await client.call_async({"function_id": "wrap::fn", "payload": {"n": 3}})

    assert asyncio.run(from_a_loop()) == {"wrapped": 6}
    # routed
    engine.deliver(invoke("wrap::fn", {"n": 5}, "r-1"))
    [answer_frame] = engine.wait_for(lambda: answers(engine, "r-1"), what="the answer")
    assert answer_frame["result"] == {"wrapped": 10}
    settle(client, engine)
    # the routed call's match is reported in an item: the answer is not the handler's output
    assert "observed" not in answer_frame
    outputs = sorted(i["output"] for i in _items(engine, "wrap::fn"))
    assert outputs == [4, 6, 10]


def test_each_observer_is_delivered_at_most_once_per_call_and_changed_calls_report_items(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)

    @client.on_observer_deliver("batch::fn")
    def per_item(call: Call[Any, Any], observers: Any, next: Any) -> Any:
        next(call, observers)
        next(call, observers)  # again: nobody twice
        for item in call.input["items"]:
            next(replace(call, input=item), observers)
        return call.output

    client.register_function("batch::fn", lambda batch: len(batch["items"]))
    _confirm(
        client,
        engine,
        "batch::fn",
        observer_info("any", "t", capture=["input"]),
        observer_info("red", "t", filters={"$and": [{"input.color": {"$eq": "red"}}]}, capture=["input"]),
        observer_info("blue", "t", filters={"$and": [{"input.color": {"$eq": "blue"}}]}, capture=["input"]),
    )
    batch = {"items": [{"color": "red"}, {"color": "blue"}, {"color": "red"}]}
    assert client.call({"function_id": "batch::fn", "payload": batch}) == 3
    settle(client, engine)
    items = _items(engine, "batch::fn")
    assert [(i["matched"], i["input"]) for i in items] == [
        (["red"], {"color": "red"}),
        (["blue"], {"color": "blue"}),
        (["any"], batch),  # the unchanged call's pooled ids, in one item with the handler's call
    ]
    assert len({i["invocation_id"] for i in items}) == 1


def test_a_hook_rejection_or_call_error_keeps_its_code(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)

    @client.on_observer_deliver("queue::on-message")
    async def ack(call: Call[Any, Any], observers: Any, next: Any) -> Any:
        if call.metadata and call.metadata.get("subscriber") == "gone":
            raise ObserveRejection("OBSERVER_NOT_ACTIVE", "subscriber removed")
        raise RuntimeError("plain hook failure")

    client.register_function("queue::on-message", lambda message: message, private=True)
    _confirm(client, engine, "queue::on-message")
    with pytest.raises(InvocationError) as raised:
        client.call({"function_id": "queue::on-message", "payload": {}, "metadata": {"subscriber": "gone"}})
    assert (raised.value.code, raised.value.message) == ("OBSERVER_NOT_ACTIVE", "subscriber removed")
    assert isinstance(raised.value.__cause__, ObserveRejection)
    with pytest.raises(InvocationError) as raised:
        client.call({"function_id": "queue::on-message", "payload": {}})
    assert (raised.value.code, raised.value.message) == ("invocation_failed", "plain hook failure")
    engine.deliver(invoke("queue::on-message", {}, "r-gone", metadata={"subscriber": "gone"}))
    [answer] = engine.wait_for(lambda: answers(engine, "r-gone"), what="the answer")
    assert answer["error"] == {"code": "OBSERVER_NOT_ACTIVE", "message": "subscriber removed"}


def test_a_handler_error_rethrown_by_the_hook_is_the_handlers_own_outcome(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)

    def fail(data: Any) -> None:
        raise ValueError("out of stock")

    @client.on_observer_deliver("fail::fn")
    async def passthrough(call: Call[Any, Any], observers: Any, next: Any) -> Any:
        assert call.status == "error" and call.error is not None and call.error["message"] == "out of stock"
        return await next(call, observers)

    client.register_function("fail::fn", fail)
    _confirm(client, engine, "fail::fn", observer_info("e", "t", on="error", capture=["error"]))
    with pytest.raises(InvocationError) as raised:
        client.call({"function_id": "fail::fn", "payload": {}})
    assert raised.value.code == "invocation_failed" and isinstance(raised.value.__cause__, ValueError)
    engine.deliver(invoke("fail::fn", {}, "r-fail"))
    [answer] = engine.wait_for(lambda: answers(engine, "r-fail"), what="the answer")
    assert answer["observed"] == ["e"]  # the error rides the answer, with its observer
    assert answer["error"]["message"] == "out of stock"


def test_a_hook_that_turns_a_handler_error_into_a_result_reports_an_item(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)

    def fail(data: Any) -> None:
        raise ValueError("boom")

    @client.on_observer_deliver("recover::fn")
    def recover(call: Call[Any, Any], observers: Any, next: Any) -> Any:
        try:
            return next(call, observers)
        except ValueError:
            return {"recovered": True}

    client.register_function("recover::fn", fail)
    _confirm(client, engine, "recover::fn", observer_info("e", "t", on="error", capture=["error"]))
    engine.deliver(invoke("recover::fn", {}, "r-recover"))
    [answer] = engine.wait_for(lambda: answers(engine, "r-recover"), what="the answer")
    assert answer["result"] == {"recovered": True} and "observed" not in answer
    [item] = engine.frames_of("invocationsobserved")[0]["items"]
    assert (item["invocation_id"], item["status"], item["matched"]) == ("r-recover", "error", ["e"])
    assert engine.frames().index(engine.frames_of("invocationsobserved")[0]) < engine.frames().index(answer)


def test_a_next_call_after_a_routed_hook_settled_delivers_nothing_and_counts_late_delivery(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    kept: list[Any] = []

    @client.on_observer_deliver("late::fn")
    def keep(call: Call[Any, Any], observers: Any, next: Any) -> Any:
        kept.append((call, observers, next))
        return call.output

    client.register_function("late::fn", lambda d: d)
    _confirm(client, engine, "late::fn", observer_info("o", "t"))
    engine.deliver(invoke("late::fn", 1, "r-late"))
    [answer] = engine.wait_for(lambda: answers(engine, "r-late"), what="the answer")
    assert "observed" not in answer
    call, observers, next = kept[0]
    assert next(call, observers) == 1
    settle(client, engine)
    assert reported_items(engine) == []
    assert client._reporter._counters.drops == {"late::fn": {"LATE_DELIVERY": 1}}


def test_the_deliver_hook_runs_in_the_functions_invocation(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    seen: list[Any] = []

    @client.on_observer_deliver("ctx::fn")
    def inside(call: Call[Any, Any], observers: Any, next: Any) -> Any:
        seen.append(client.current_invocation())
        client.call({"function_id": "elsewhere::fn", "payload": {}, "action": CallAction.Void()})
        return next(call, observers)

    client.register_function("ctx::fn", lambda d: d)
    _confirm(client, engine, "ctx::fn")
    engine.deliver(invoke("ctx::fn", {}, "r-ctx", caller={"worker_id": "w", "worker_name": "caller"}))
    engine.wait_for(lambda: answers(engine, "r-ctx"), what="the answer")
    [inv] = seen
    assert (inv.function_id, inv.invocation_id) == ("ctx::fn", "r-ctx")
    [call] = engine.wait_for(
        lambda: [f for f in engine.frames_of("invokefunction") if f["function_id"] == "elsewhere::fn"], what="it"
    )
    assert call["caller"] == {
        "function_id": "ctx::fn",
        "invocation_id": "r-ctx",
        "parent": {"worker_id": "w", "worker_name": "caller"},
    }


def test_a_routed_call_with_no_observers_and_no_hook_sends_only_its_answer(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    client.register_function("quiet::fn", lambda d: d)
    _confirm(client, engine, "quiet::fn")
    engine.clear()
    engine.deliver(invoke("quiet::fn", 7, "r-quiet"))
    [answer] = engine.wait_for(lambda: answers(engine, "r-quiet"), what="the answer")
    settle(client, engine)
    assert engine.frames() == [answer] and "observed" not in answer


# ── keyed ──────────────────────────────────────────────────────────────────


def _row_key(db: str, table: str, op: str) -> str:
    return f"{db}|{table}|{op}"


def _row_changed(client: III, engine: FakeEngine) -> list[str]:
    """The database worker's source: observers filed under ``db|table|op`` with wildcards. Returns the ids
    the observer key function was called for."""
    key_calls: list[str] = []

    def observer_keys(obs: Observer) -> list[str]:
        key_calls.append(obs.id)
        args = obs.args
        return [_row_key(args["db"], args.get("table", "*"), op) for op in args.get("ops", ["*"])]

    def call_keys(call: Call[Any, Any]) -> list[str]:
        change = call.input
        return [_row_key(change["db"], t, op) for t in (change["table"], "*") for op in (change["op"], "*")]

    client.register_function("database::on-row-changed", lambda change: change, private=True)
    client.on_observer_deliver("database::on-row-changed", keyed(observer=observer_keys, call=call_keys))
    _confirm(
        client,
        engine,
        "database::on-row-changed",
        observer_info("orders-insert", "t", args={"db": "primary", "table": "orders", "ops": ["insert"]}),
        observer_info("orders-any", "t", args={"db": "primary", "table": "orders"}),
        observer_info("every-insert", "t", args={"db": "primary", "ops": ["insert", "update"]}),
        observer_info("other-db", "t", args={"db": "replica"}),
    )
    return key_calls


def test_keyed_delivers_to_the_observers_filed_under_the_calls_keys(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    key_calls = _row_changed(client, engine)
    assert sorted(key_calls) == ["every-insert", "orders-any", "orders-insert", "other-db"]  # once each
    for change in (
        {"db": "primary", "table": "orders", "op": "insert"},
        {"db": "primary", "table": "orders", "op": "delete"},
        {"db": "primary", "table": "users", "op": "update"},
        {"db": "primary", "table": "users", "op": "delete"},
    ):
        client.call({"function_id": "database::on-row-changed", "payload": change})
    settle(client, engine)
    assert [sorted(i["matched"]) for i in _items(engine, "database::on-row-changed")] == [
        ["every-insert", "orders-any", "orders-insert"],
        ["orders-any"],
        ["every-insert"],
    ]
    assert sorted(key_calls) == ["every-insert", "orders-any", "orders-insert", "other-db"]  # computed once


def test_keyed_is_observed_and_the_index_under_add_and_remove(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    _row_changed(client, engine)
    fid = "database::on-row-changed"
    users_delete = {"input": {"db": "primary", "table": "users", "op": "delete"}}
    assert client.is_observed(fid, {"input": {"db": "replica", "table": "x", "op": "delete"}}) is True
    assert client.is_observed(fid, users_delete) is False
    assert client.is_observed(fid) is True  # without a call: any observer
    engine.deliver(observer_added(fid, 1, observer_info("users", "t", args={"db": "primary", "table": "users"})))
    engine.wait_for(lambda: "users" in {a["id"] for a in acks(engine)}, what="the ack")
    assert client.is_observed(fid, users_delete) is True
    client.call({"function_id": fid, "payload": users_delete["input"]})
    engine.deliver(observer_removed(fid, 2, "users"))
    wait_until(lambda: not client.is_observed(fid, users_delete), what="it to be unfiled")
    client.call({"function_id": fid, "payload": users_delete["input"]})
    settle(client, engine)
    assert [i["matched"] for i in _items(engine, fid)] == [["users"]]


def test_keyed_set_once_observers_are_active_files_them_and_a_failing_key_rejects(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    ref = client.register_function("cron::on-tick", lambda tick: tick, private=True)
    _confirm(
        client,
        engine,
        "cron::on-tick",
        observer_info("ok", "t", args={"expression": "*/5"}),
        observer_info("bad", "t", args={}),
    )
    engine.clear()
    ref.on_observer_deliver(keyed(observer=lambda o: o.args["expression"], call=lambda c: c.input["expression"]))
    [ack] = engine.wait_for(lambda: acks(engine), what="the rejection")
    assert (ack["id"], ack["error"]["code"]) == ("bad", "OBSERVE_REJECTED")
    assert ref.is_observed({"input": {"expression": "*/5"}}) is True
    assert ref.is_observed({"input": {"expression": "*/7"}}) is False
    # an observer added later whose key fails is rejected before it is accepted
    engine.deliver(observer_added("cron::on-tick", 1, observer_info("later", "t", args={"expr": "typo"})))
    engine.wait_for(lambda: len(acks(engine)) == 2, what="its rejection")
    assert acks(engine)[1]["error"]["code"] == "OBSERVE_REJECTED"
    # back to iii's delivery: every active observer
    ref.on_observer_deliver(None)
    assert ref.is_observed({"input": {"expression": "*/7"}}) is True


def test_keyed_called_directly_picks_from_the_observers_it_is_given(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    by_expression = keyed(observer=lambda o: o.args["expression"], call=lambda c: c.input["expression"])

    @client.on_observer_deliver("composed::fn")
    def composed(call: Call[Any, Any], observers: Any, next: Any) -> Any:
        return by_expression(call, observers, next)

    client.register_function("composed::fn", lambda tick: tick)
    _confirm(
        client,
        engine,
        "composed::fn",
        observer_info("a", "t", args={"expression": "x"}),
        observer_info("b", "t", args={"expression": "y"}),
    )
    client.call({"function_id": "composed::fn", "payload": {"expression": "y"}})
    settle(client, engine)
    assert [i["matched"] for i in _items(engine, "composed::fn")] == [["b"]]
    with pytest.raises(TypeError):
        keyed(observer=lambda o: 1, call=lambda c: "k").observer_keys(client.observers("composed::fn")[0])  # type: ignore[arg-type,return-value]


# ── Observer.matches, is_observed, observers ───────────────────────────────


def test_matches_checks_on_and_filters_of_a_call_or_a_partial_one(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    client.register_function("m::fn", lambda d: d)
    _confirm(
        client,
        engine,
        "m::fn",
        observer_info("big", "t", filters={"$and": [{"input.n": {"$gt": 1}}]}),
        observer_info("err", "t", on="error"),
    )
    big, err = client.observers("m::fn")
    ok_call = Call(function_id="m::fn", invocation_id="i", status="success", input={"n": 2}, output=2)
    failed = replace(ok_call, status="error", output=None, error={"code": "x", "message": "y"})
    assert big.matches(ok_call) is True and big.matches(failed) is False
    assert err.matches(failed) is True and err.matches(ok_call) is False
    assert big.matches({"input": {"n": 2}}) is True and big.matches({"input": {"n": 0}}) is False
    assert err.matches({"input": {}}) is True  # no status: on is not checked
    assert client.is_observed("m::fn") is True and client.is_observed("nobody::fn") is False
    assert client.observers("nobody::fn") == []


# ── calls through an observer ──────────────────────────────────────────────


def _answer_observer_calls(engine: FakeEngine, result: Any = None, error: dict[str, str] | None = None) -> list[Any]:
    calls: list[dict[str, Any]] = []

    def on_frame(frame: dict[str, Any]) -> list[dict[str, Any]] | None:
        if frame.get("type") != "invokefunction" or "activation" not in frame:
            return None
        calls.append(frame)
        if "invocation_id" not in frame:
            return None
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


def _router(client: III, engine: FakeEngine) -> None:
    routes: dict[str, Observer] = {}
    client.on_observer_added("http::on-request", lambda obs: routes.__setitem__(obs.args["path"], obs))
    client.on_observer_removed("http::on-request", lambda obs: routes.pop(obs.args["path"], None))

    @client.on_observer_deliver("http::on-request")
    async def route(call: Call[Any, Any], observers: Any, next: Any) -> Any:
        obs = routes.get(call.input["path"])
        if obs is None:
            return {"status_code": 404}
        if not obs.matches(call):
            return {"status_code": 422}
        return await client.call_async({"observer": obs, "payload": call.input})

    client.register_function("http::on-request", lambda request: request, private=True)
    _confirm(
        client,
        engine,
        "http::on-request",
        observer_info(
            "route-1", "users::get", args={"path": "/users"}, filters={"$and": [{"input.tenant": {"$eq": "acme"}}]}
        ),
    )


def test_a_hook_calls_through_the_observer_and_the_targets_result_is_the_response(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    calls = _answer_observer_calls(engine, result={"status_code": 200})
    _router(client, engine)
    request = {"path": "/users", "tenant": "acme"}
    assert client.call({"function_id": "http::on-request", "payload": request}) == {"status_code": 200}
    assert client.call({"function_id": "http::on-request", "payload": {"path": "/nope"}}) == {"status_code": 404}
    assert client.call({"function_id": "http::on-request", "payload": {"path": "/users"}}) == {"status_code": 422}
    [frame] = calls
    assert frame["function_id"] == "users::get"
    assert frame["activation"] == {"observer_id": "route-1"}
    assert frame["data"] == request
    assert "namespace" not in frame and "metadata" not in frame
    assert frame["caller"]["function_id"] == "http::on-request"
    assert "observe_depth" not in frame  # 0 is omitted


def test_a_refused_call_through_an_observer_raises_the_engines_code(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    _answer_observer_calls(engine, error={"code": "OVERFLOW", "message": "max_in_flight reached"})
    _router(client, engine)
    obs = client.observers("http::on-request")[0]
    with pytest.raises(InvocationError) as raised:
        client.call({"observer": obs, "payload": {}})
    assert raised.value.code == "OVERFLOW"
    # raised in the deliver hook: the function's caller gets the same code, local or routed
    request = {"path": "/users", "tenant": "acme"}
    with pytest.raises(InvocationError) as raised:
        client.call({"function_id": "http::on-request", "payload": request})
    assert (raised.value.code, raised.value.function_id) == ("OVERFLOW", "http::on-request")
    engine.deliver(invoke("http::on-request", request, "r-overflow"))
    [answer] = engine.wait_for(lambda: answers(engine, "r-overflow"), what="the answer")
    assert answer["error"] == {"code": "OVERFLOW", "message": "max_in_flight reached"}


def test_a_call_through_an_observer_takes_no_function_id_namespace_metadata_or_enqueue(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    _router(client, engine)
    obs = client.observers("http::on-request")[0]
    for extra in ({"function_id": "x::y"}, {"namespace": "acme"}, {"metadata": {"a": 1}}):
        with pytest.raises(ValueError):
            client.call({"observer": obs, "payload": {}, **extra})
    with pytest.raises(ValueError):
        client.call({"observer": obs, "payload": {}, "action": CallAction.Enqueue(queue="q")})
    with pytest.raises(TypeError):
        client.call({"observer": "route-1", "payload": {}})
    with pytest.raises(ValueError):
        client.call({"payload": {}})


def test_accepted_resolves_once_the_observer_is_acked_and_raises_when_it_is_rejected(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    calls = _answer_observer_calls(engine)
    release = threading.Event()
    got: list[Observer] = []
    initial: list[str] = []

    async def send_initial_state(obs: Observer) -> None:
        await obs.accepted_async()
        await client.call_async({"observer": obs, "payload": {"state": "initial"}, "action": CallAction.Void()})
        initial.append(obs.id)

    @client.on_observer_added("stock::on-changed")
    async def on_added(obs: Observer) -> None:
        got.append(obs)
        if obs.args.get("refuse"):
            raise ObserveRejection("REFUSED", "no")
        asyncio.get_running_loop().create_task(send_initial_state(obs))
        while not release.is_set():
            await asyncio.sleep(0.005)

    client.register_function("stock::on-changed", lambda e: e, private=True)
    engine.deliver(function_registered("stock::on-changed", [observer_info("o", "t")], revision=1))
    wait_until(lambda: got, what="the hook")
    with pytest.raises(TimeoutError):
        got[0].accepted(timeout=0.05)
    assert calls == []  # nothing goes through it before its ack
    release.set()
    got[0].accepted(timeout=5)
    wait_until(lambda: initial == ["o"], what="the initial state")
    frames = engine.frames()
    ack_at = next(i for i, f in enumerate(frames) if f["type"] == "observerack")
    call_at = next(i for i, f in enumerate(frames) if f["type"] == "invokefunction" and f.get("activation"))
    assert ack_at < call_at
    engine.deliver(observer_added("stock::on-changed", 2, observer_info("r", "t", args={"refuse": True})))
    wait_until(lambda: len(got) == 2, what="the second hook")
    with pytest.raises(ObserveRejection) as raised:
        got[1].accepted(timeout=5)
    assert raised.value.code == "REFUSED"

    async def on_the_sdk_loop() -> None:
        with pytest.raises(RuntimeError):
            got[0].accepted()

    client._run_on_loop(on_the_sdk_loop())


# ── private and output_is_input ────────────────────────────────────────────


def test_channel_refs_are_resolved_and_the_rest_of_the_value_is_kept(engine: FakeEngine, connect: Any) -> None:
    from iii.channels import ChannelReader

    client = connect(engine)
    plain = {"a": [1, {"b": 2}], "c": (3,)}
    assert client._resolve_channels(plain) is plain
    ref = {"channel_id": "ch-1", "access_key": "k", "direction": "read"}
    keep = {"x": 1}
    value = {"body": {"stream": ref}, "keep": keep, "list": [ref, 2]}
    resolved = client._resolve_channels(value)
    assert resolved is not value and value["body"]["stream"] is ref  # the caller's value is untouched
    assert isinstance(resolved["body"]["stream"], ChannelReader) and isinstance(resolved["list"][0], ChannelReader)
    assert resolved["keep"] is keep and resolved["list"][1] == 2


def test_a_private_function_passes_input_and_result_by_reference(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    seen: list[Any] = []

    def source(event: dict[str, Any]) -> dict[str, Any]:
        seen.append(event)
        return event

    client.register_function("events::on-thing", source, private=True)
    client.register_function("events::copied", source)
    _confirm(client, engine, "events::on-thing")
    _confirm(client, engine, "events::copied")
    event = {"n": 1}
    assert client.call({"function_id": "events::on-thing", "payload": event}) is event
    assert seen[-1] is event
    copied = client.call({"function_id": "events::copied", "payload": event})
    assert copied == event and copied is not event and seen[-1] is not event


def test_an_event_source_reports_the_event_once_with_output_is_input(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    client.register_function("events::on-thing", lambda event: event, private=True)
    client.register_function("events::other", lambda event: dict(event), private=True)
    _confirm(client, engine, "events::on-thing", observer_info("o", "t"))
    _confirm(client, engine, "events::other", observer_info("p", "t"))
    client.call({"function_id": "events::on-thing", "payload": {"n": 1}})
    client.call({"function_id": "events::other", "payload": {"n": 2}})
    settle(client, engine)
    [source] = _items(engine, "events::on-thing")
    assert source["input"] == {"n": 1} and source["output_is_input"] is True and "output" not in source
    [other] = _items(engine, "events::other")
    assert other["output"] == {"n": 2} and "output_is_input" not in other


def test_an_engine_without_observe_v2_gets_output_not_output_is_input(engine_factory: Any, connect: Any) -> None:
    engine = engine_factory(V1_FEATURES)
    client = connect(engine)
    client.register_function("events::on-thing", lambda event: event)
    _confirm(client, engine, "events::on-thing", observer_info("o", "t"))
    client.call({"function_id": "events::on-thing", "payload": {"n": 1}})
    settle(client, engine)
    [item] = _items(engine, "events::on-thing")
    assert item["output"] == {"n": 1} and "output_is_input" not in item


# ── engines without observe.v2 ─────────────────────────────────────────────


def test_functions_that_need_observe_v2_wait_for_it_and_stay_out_of_a_v1_engine(
    engine_factory: Any, connect: Any, caplog: pytest.LogCaptureFixture
) -> None:
    engine = engine_factory(V1_FEATURES)
    with caplog.at_level(logging.ERROR, logger="iii.iii"):
        client = connect(engine)
        client.register_function("plain::fn", lambda d: d)
        client.register_function("args::fn", lambda d: d, args_format={"type": "object"})
        client.register_function("private::fn", lambda d: d, private=True)
        client.register_function("sealed::fn", lambda d: d, observable=False)
        client.on_observer_deliver("hooked::fn", lambda call, observers, next: next(call, observers))
        client.register_function("hooked::fn", lambda d: d)
        settle(client, engine)
        engine.drop()
        wait_until(lambda: len(engine.connections) == 2 and client._registered_on_connection, what="the reconnect")
        settle(client, engine)
    assert sorted(f["id"] for f in engine.frames_of("registerfunction")) == ["plain::fn", "plain::fn"]
    unsupported = [r.getMessage() for r in caplog.records if "UNSUPPORTED" in r.getMessage()]
    assert sorted(m.split("'")[1] for m in unsupported) == ["args::fn", "hooked::fn", "private::fn", "sealed::fn"]
    # calls and observe keep working; routed calls are the engine's to deliver
    client.register_function("observed::fn", lambda d: d)
    _confirm(client, engine, "observed::fn", observer_info("o", "t"))
    engine.deliver(invoke("observed::fn", 1, "r-v1"))
    [answer] = engine.wait_for(lambda: answers(engine, "r-v1"), what="the answer")
    assert answer["result"] == 1 and "observed" not in answer


def test_registration_needing_observe_v2_goes_after_workerregistered_on_every_connection(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    client.register_function("cron::on-tick", lambda t: t, private=True)
    engine.wait_for(lambda: engine.frames_of("registerfunction"), what="the registration")
    engine.drop()
    wait_until(lambda: len(engine.connections) == 2 and client._registered_on_connection, what="the reconnect")
    settle(client, engine)
    second = engine.frames(1)
    registered_at = next(i for i, f in enumerate(second) if f["type"] == "registerfunction")
    register_call_at = next(
        i for i, f in enumerate(second) if f.get("function_id") == "engine::workers::register"
    )
    assert register_call_at < registered_at  # held until the engine said what it supports
    assert [f["id"] for f in second if f["type"] == "registerfunction"] == ["cron::on-tick"]


def test_a_deliver_hook_set_after_registering_on_a_v1_engine_unregisters_the_function(
    engine_factory: Any, connect: Any
) -> None:
    engine = engine_factory(V1_FEATURES)
    client = connect(engine)
    ref = client.register_function("late-hook::fn", lambda d: d)
    engine.wait_for(lambda: engine.frames_of("registerfunction"), what="the plain registration")
    ref.on_observer_deliver(lambda call, observers, next: next(call, observers))
    [frame] = engine.wait_for(lambda: engine.frames_of("unregisterfunction"), what="the unregistration")
    assert frame["id"] == "late-hook::fn"


# ── standby ────────────────────────────────────────────────────────────────


def test_standby_retries_a_conflicting_registration_until_the_id_is_free(
    engine: FakeEngine, connect: Any, caplog: pytest.LogCaptureFixture
) -> None:
    client = connect(engine)
    client._standby_retry_s = 0.02
    conflicts = {"left": 2}

    def answer(frame: dict[str, Any]) -> list[dict[str, Any]] | None:
        if frame.get("type") != "registerfunction" or frame["id"] != "cron::on-tick":
            return None
        if conflicts["left"] > 0:
            conflicts["left"] -= 1
            return [rejected("cron::on-tick", owner="host-1")]
        return [function_registered("cron::on-tick", [observer_info("o", "t")])]

    engine.on_frame = answer
    started: list[str] = []
    client.on_observer_added("cron::on-tick", lambda obs: started.append(obs.id))
    with caplog.at_level(logging.INFO, logger="iii.iii"):
        ref = client.register_function("cron::on-tick", lambda t: t, private=True, standby=True)
        engine.wait_for(lambda: acks(engine), what="the take-over")
    assert len(engine.frames_of("registerfunction")) == 3
    assert started == ["o"]  # hooks run when functionregistered arrives
    assert ref.is_observed() is True
    standing = [r for r in caplog.records if "standing by" in r.getMessage()]
    assert len(standing) == 1 and "host-1" in standing[0].getMessage()


def test_unregister_stops_a_standby_and_a_non_standby_conflict_is_not_retried(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    client._standby_retry_s = 0.02

    def conflict(frame: dict[str, Any]) -> list[dict[str, Any]] | None:
        if frame.get("type") == "registerfunction":
            return [rejected(frame["id"])]
        return None

    engine.on_frame = conflict
    ref = client.register_function("standby::fn", lambda d: d, standby=True)
    client.register_function("plain::fn", lambda d: d)
    engine.wait_for(
        lambda: len([f for f in engine.frames_of("registerfunction") if f["id"] == "standby::fn"]) >= 2,
        what="a retry",
    )
    ref.unregister()
    settle(client, engine)
    sent = len(engine.frames_of("registerfunction"))
    time.sleep(0.1)  # several retry periods
    assert len(engine.frames_of("registerfunction")) == sent
    assert [f["id"] for f in engine.frames_of("registerfunction")].count("plain::fn") == 1
