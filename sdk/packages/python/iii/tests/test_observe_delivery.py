"""Delivery in the worker that hosts a function: matching
(``on_observer_match``, by predicate and by keys), iii's delivery, an
``on_observer_deliver`` hook, ``is_observed``, calls through an observer,
``private`` and ``output_is_input``, older engines and ``standby``. Against a
fake engine."""

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

from iii import Call, CallAction, InvocationError, Observer, ObserveRejection
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


# ── iii's delivery ─────────────────────────────────────────────────────────


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


def test_a_routed_call_lists_what_iii_delivered_on_its_answer_and_reports_nothing(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)

    def charge(data: dict[str, Any]) -> dict[str, Any]:
        if data["amount"] < 0:
            raise ValueError("refused")
        return {"charged": data["amount"]}

    client.register_function("payments::charge", charge)
    _confirm(
        client,
        engine,
        "payments::charge",
        observer_info("big", "t", filters={"$and": [{"input.amount": {"$gte": 100}}]}),
        observer_info("errors", "t", on="error"),
    )
    engine.deliver(invoke("payments::charge", {"amount": 500}, "r-ok"))
    engine.deliver(invoke("payments::charge", {"amount": -1}, "r-error"))
    engine.deliver(invoke("payments::charge", {"amount": 5}, "r-none"))
    [ok] = engine.wait_for(lambda: answers(engine, "r-ok"), what="the answer")
    [failed] = engine.wait_for(lambda: answers(engine, "r-error"), what="the error answer")
    [none] = engine.wait_for(lambda: answers(engine, "r-none"), what="the unmatched answer")
    assert (ok["result"], ok["observed"]) == ({"charged": 500}, ["big"])
    # the handler's error rides the answer, with the observers delivered for it
    assert (failed["error"]["message"], failed["observed"]) == ("refused", ["errors"])
    assert "observed" not in none
    settle(client, engine)
    assert reported_items(engine) == []


# ── on_observer_match: a predicate ─────────────────────────────────────────


def _cron(client: III, engine: FakeEngine) -> list[tuple[str, Any]]:
    """``cron::on-tick``: an observer of an expression gets that expression's ticks. Returns each predicate call."""
    asked: list[tuple[str, Any]] = []

    def wants(obs: Observer, call: Call[Any, Any]) -> bool:
        asked.append((obs.id, call.input["n"]))
        return bool(obs.args["expression"] == call.input["expression"])

    client.register_function("cron::on-tick", lambda tick: tick, private=True).on_observer_match(wants)
    _confirm(
        client,
        engine,
        "cron::on-tick",
        observer_info("five", "t", args={"expression": "*/5"}),
        observer_info("ten", "t", args={"expression": "*/10"}),
        observer_info("ten-even", "t", args={"expression": "*/10"}, filters={"$and": [{"input.n": {"$eq": 2}}]}),
    )
    return asked


def test_a_predicate_runs_once_per_active_observer_per_call_before_filters(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    asked = _cron(client, engine)
    tick = {"expression": "*/10", "n": 1}
    assert client.call({"function_id": "cron::on-tick", "payload": tick}) is tick  # private: by reference
    client.call({"function_id": "cron::on-tick", "payload": {"expression": "*/10", "n": 2}})
    engine.deliver(invoke("cron::on-tick", {"expression": "*/5", "n": 3}, "r-tick"))
    [answer] = engine.wait_for(lambda: answers(engine, "r-tick"), what="the answer")
    settle(client, engine)
    # every active observer, filters or not, once per call
    assert sorted(asked) == sorted((o, n) for o in ("five", "ten", "ten-even") for n in (1, 2, 3))
    # then iii's delivery applies the filters of the observers that want the call
    assert [sorted(i["matched"]) for i in _items(engine, "cron::on-tick")] == [["ten"], ["ten", "ten-even"]]
    assert answer["observed"] == ["five"]


def test_a_predicate_that_raises_is_no_match_and_is_logged_once_per_function(
    engine: FakeEngine, connect: Any, caplog: pytest.LogCaptureFixture
) -> None:
    client = connect(engine)

    @client.on_observer_match("flaky::fn")
    def wants(obs: Observer, call: Call[Any, Any]) -> bool:
        if obs.id == "broken":
            raise KeyError("expression")
        return True

    client.register_function("flaky::fn", lambda d: d)
    _confirm(client, engine, "flaky::fn", observer_info("broken", "t"), observer_info("fine", "t"))
    with caplog.at_level(logging.ERROR, logger="iii.observe"):
        assert client.call({"function_id": "flaky::fn", "payload": 1}) == 1
        assert client.call({"function_id": "flaky::fn", "payload": 2}) == 2
        settle(client, engine)
    assert [i["matched"] for i in _items(engine, "flaky::fn")] == [["fine"], ["fine"]]
    logged = [r for r in caplog.records if "on_observer_match of flaky::fn" in r.getMessage()]
    assert len(logged) == 1


# ── on_observer_match: keys ────────────────────────────────────────────────


def _row_key(db: str, table: str, op: str) -> str:
    return f"{db}|{table}|{op}"


def _row_changed(client: III, engine: FakeEngine) -> tuple[list[str], list[Any]]:
    """The database worker's source: observers filed under ``db|table|op`` with wildcards. Returns the ids the
    observer key function was called for, and the inputs the call key function was called for."""
    observer_calls: list[str] = []
    call_calls: list[Any] = []

    def observer_keys(obs: Observer) -> list[str]:
        observer_calls.append(obs.id)
        args = obs.args
        return [_row_key(args["db"], args.get("table", "*"), op) for op in args.get("ops", ["*"])]

    def call_keys(call: Call[Any, Any]) -> list[str]:
        change = call.input
        call_calls.append(change)
        return [_row_key(change["db"], t, op) for t in (change["table"], "*") for op in (change["op"], "*")]

    client.register_function("database::on-row-changed", lambda change: change, private=True)
    client.on_observer_match("database::on-row-changed", observer=observer_keys, call=call_keys)
    _confirm(
        client,
        engine,
        "database::on-row-changed",
        observer_info("orders-insert", "t", args={"db": "primary", "table": "orders", "ops": ["insert"]}),
        observer_info("orders-any", "t", args={"db": "primary", "table": "orders"}),
        observer_info("every-insert", "t", args={"db": "primary", "ops": ["insert", "update"]}),
        observer_info("other-db", "t", args={"db": "replica"}),
    )
    return observer_calls, call_calls


def test_keys_match_the_observers_filed_under_the_calls_keys(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    observer_calls, call_calls = _row_changed(client, engine)
    assert sorted(observer_calls) == ["every-insert", "orders-any", "orders-insert", "other-db"]  # once each
    changes = [
        {"db": "primary", "table": "orders", "op": "insert"},
        {"db": "primary", "table": "orders", "op": "delete"},
        {"db": "primary", "table": "users", "op": "update"},
        {"db": "primary", "table": "users", "op": "delete"},
    ]
    for change in changes:
        client.call({"function_id": "database::on-row-changed", "payload": change})
    engine.deliver(invoke("database::on-row-changed", {"db": "replica", "table": "x", "op": "delete"}, "r-row"))
    [answer] = engine.wait_for(lambda: answers(engine, "r-row"), what="the answer")
    settle(client, engine)
    assert [sorted(i["matched"]) for i in _items(engine, "database::on-row-changed")] == [
        ["every-insert", "orders-any", "orders-insert"],
        ["orders-any"],
        ["every-insert"],
    ]
    assert answer["observed"] == ["other-db"]
    assert sorted(observer_calls) == ["every-insert", "orders-any", "orders-insert", "other-db"]  # computed once
    assert len(call_calls) == len(changes) + 1  # once per call


def test_keys_is_observed_and_the_index_under_add_and_remove(engine: FakeEngine, connect: Any) -> None:
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
    engine.deliver(observer_removed(fid, 3, "orders-any"))
    orders_delete = {"input": {"db": "primary", "table": "orders", "op": "delete"}}
    wait_until(lambda: not client.is_observed(fid, orders_delete), what="orders-any to be unfiled")
    orders_insert = {"db": "primary", "table": "orders", "op": "insert"}
    client.call({"function_id": fid, "payload": orders_insert})
    settle(client, engine)
    assert [sorted(i["matched"]) for i in _items(engine, fid)] == [["users"], ["every-insert", "orders-insert"]]


def test_a_call_is_matched_against_the_active_set_it_started_with(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    _row_changed(client, engine)
    fid = "database::on-row-changed"
    state = client._functions[fid].observe
    started = state.active
    call = Call(
        function_id=fid, invocation_id="i", status="success", input={"db": "replica", "table": "x", "op": "delete"}
    )
    engine.deliver(observer_removed(fid, 1, "other-db"))
    wait_until(lambda: state.active is not started, what="the removal")
    assert [o.id for o in state.matching(started, call)] == ["other-db"]  # the snapshot is never changed
    assert state.matching(state.active, call) == ()
    assert [o.id for o in started.observers] == ["orders-insert", "orders-any", "every-insert", "other-db"]


@pytest.mark.parametrize("hooked", [False, True])
def test_an_observer_accepted_while_a_call_runs_does_not_match_it(
    engine: FakeEngine, connect: Any, hooked: bool
) -> None:
    """Decision 20: a call matched by keys matches only the observers active
    when it started. One accepted while the handler runs is neither delivered
    the call nor handed to the deliver hook."""
    client = connect(engine)
    fid = "kd::fn"
    gate = threading.Event()
    entered = threading.Event()

    async def handler(data: Any) -> Any:
        entered.set()
        while not gate.is_set():
            await asyncio.sleep(0.005)
        return data

    client.on_observer_match(fid, observer=lambda obs: obs.args["kind"], call=lambda call: call.input["kind"])
    if hooked:
        client.on_observer_deliver(fid, lambda call, observers: [o.id for o in observers])
    client.register_function(fid, handler)
    _confirm(client, engine, fid, observer_info("a", "t", args={"kind": "x"}))
    engine.deliver(invoke(fid, {"kind": "x"}, "r-during"))
    assert entered.wait(5)
    engine.deliver(observer_added(fid, 1, observer_info("b", "t", args={"kind": "x"})))
    engine.wait_for(lambda: "b" in {a["id"] for a in acks(engine)}, what="b's ack")
    assert sorted(o.id for o in client.observers(fid)) == ["a", "b"]
    gate.set()
    [answer] = engine.wait_for(lambda: answers(engine, "r-during"), what="the answer")
    if hooked:
        assert answer["result"] == ["a"]
    else:
        assert answer["observed"] == ["a"]


def test_keys_set_while_observers_are_active_file_them_and_a_failing_key_rejects(
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
    keys = ref.on_observer_match(observer=lambda o: o.args["expression"], call=lambda c: c.input["expression"])
    assert client._match_hooks["cron::on-tick"] is keys
    [ack] = engine.wait_for(lambda: acks(engine), what="the rejection")
    assert (ack["id"], ack["error"]["code"]) == ("bad", "OBSERVE_REJECTED")
    assert "could not be computed" in ack["error"]["message"]
    assert ref.is_observed({"input": {"expression": "*/5"}}) is True
    assert ref.is_observed({"input": {"expression": "*/7"}}) is False
    # an observer added later whose key raises is rejected before it is accepted
    engine.deliver(observer_added("cron::on-tick", 1, observer_info("later", "t", args={"expr": "typo"})))
    engine.wait_for(lambda: len(acks(engine)) == 2, what="its rejection")
    assert acks(engine)[1]["error"]["code"] == "OBSERVE_REJECTED"
    # without a match hook every active observer wants every call
    ref.on_observer_match(None)
    wait_until(lambda: client._functions["cron::on-tick"].observe.active.index is None, what="the rebuild")
    assert ref.is_observed({"input": {"expression": "*/7"}}) is True
    assert [o.id for o in ref.observers()] == ["ok"]


def test_a_key_function_that_rejects_its_observer_does_not_block_the_sdk(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)

    def observer_keys(obs: Observer) -> str:
        if obs.args.get("refuse"):
            obs.reject("REFUSED", "not this one")
        return str(obs.args["topic"])

    client.on_observer_match("topics::fn", observer=observer_keys, call=lambda call: call.input["topic"])
    ref = client.register_function("topics::fn", lambda d: d)
    _confirm(
        client,
        engine,
        "topics::fn",
        observer_info("kept", "t", args={"topic": "a"}),
        observer_info("refused", "t", args={"topic": "a", "refuse": True}),
    )
    assert [o.id for o in ref.observers()] == ["kept"]
    assert {(a["id"], (a.get("error") or {}).get("code")) for a in acks(engine)} == {
        ("kept", None),
        ("refused", "REFUSED"),
    }
    # the same while the index is rebuilt for observers already active
    engine.clear()
    rejecting = {"on": False}

    def reject_on_rebuild(obs: Observer) -> str:
        if rejecting["on"]:
            obs.reject("REBUILT", "refused on rebuild")
        return str(obs.args["topic"])

    rejecting["on"] = True
    ref.on_observer_match(observer=reject_on_rebuild, call=lambda call: call.input["topic"])
    [ack] = engine.wait_for(lambda: acks(engine), what="the late rejection")
    assert (ack["id"], ack["error"]["code"]) == ("kept", "REBUILT")
    assert ref.observers() == [] and ref.is_observed({"input": {"topic": "a"}}) is False


def test_a_call_key_function_that_raises_matches_nothing_and_is_logged_once(
    engine: FakeEngine, connect: Any, caplog: pytest.LogCaptureFixture
) -> None:
    client = connect(engine)
    ref = client.register_function("strict::fn", lambda d: d)
    ref.on_observer_match(observer=lambda obs: "k", call=lambda call: call.input["key"])
    _confirm(client, engine, "strict::fn", observer_info("o", "t"))
    with caplog.at_level(logging.ERROR, logger="iii.observe"):
        assert ref.is_observed({"input": {}}) is False
        client.call({"function_id": "strict::fn", "payload": {}})
        client.call({"function_id": "strict::fn", "payload": {"key": "k"}})
        settle(client, engine)
    assert [i["matched"] for i in _items(engine, "strict::fn")] == [["o"]]
    assert len([r for r in caplog.records if "on_observer_match of strict::fn" in r.getMessage()]) == 1


def test_keys_must_be_strings(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    ref = client.register_function("typed::fn", lambda d: d)
    ref.on_observer_match(observer=lambda obs: 7, call=lambda call: "k")  # type: ignore[arg-type,return-value]
    engine.deliver(function_registered("typed::fn", [observer_info("o", "t")]))
    [ack] = engine.wait_for(lambda: acks(engine), what="the rejection")
    assert ack["error"]["code"] == "OBSERVE_REJECTED" and "string" in ack["error"]["message"]


# ── on_observer_deliver ────────────────────────────────────────────────────


@pytest.mark.parametrize("hook_kind", ["sync", "async"])
def test_a_deliver_hook_gets_the_call_and_the_observers_and_its_value_is_what_the_caller_gets(
    engine: FakeEngine, connect: Any, hook_kind: str
) -> None:
    client = connect(engine)
    seen: list[tuple[Any, Any, list[str]]] = []

    if hook_kind == "async":

        async def answer(call: Call[Any, Any], observers: Any) -> Any:
            seen.append((call.input, call.output, [o.id for o in observers]))
            return {"wrapped": call.output}

    else:

        def answer(call: Call[Any, Any], observers: Any) -> Any:
            seen.append((call.input, call.output, [o.id for o in observers]))
            return {"wrapped": call.output}

    client.on_observer_deliver("wrap::fn", answer)
    client.register_function("wrap::fn", lambda data: data["n"] * 2)
    _confirm(client, engine, "wrap::fn", observer_info("a", "t"), observer_info("b", "t", on="error"))
    assert client.call({"function_id": "wrap::fn", "payload": {"n": 2}}) == {"wrapped": 4}

    async def from_a_loop() -> Any:
        return await client.call_async({"function_id": "wrap::fn", "payload": {"n": 3}})

    assert asyncio.run(from_a_loop()) == {"wrapped": 6}
    engine.deliver(invoke("wrap::fn", {"n": 5}, "r-1"))
    [answer_frame] = engine.wait_for(lambda: answers(engine, "r-1"), what="the answer")
    assert answer_frame["result"] == {"wrapped": 10}
    # the hook delivers: iii delivers nothing for the function, local or routed
    assert "observed" not in answer_frame
    settle(client, engine)
    assert reported_items(engine) == [] and engine.frames_of("invocationsobserved") == []
    # every observer, `on` and filters unapplied: the hook applies them with obs.matches
    assert seen == [({"n": n}, n * 2, ["a", "b"]) for n in (2, 3, 5)]


def test_a_deliver_hook_receives_only_the_observers_that_matched(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    seen: list[list[str]] = []
    client.on_observer_match("queue::on-message", observer=lambda obs: obs.id, call=lambda c: c.metadata["subscriber"])

    @client.on_observer_deliver("queue::on-message")
    def ack(call: Call[Any, Any], observers: Any) -> Any:
        seen.append([o.id for o in observers])
        return None if observers else "unrouted"

    client.register_function("queue::on-message", lambda message: message, private=True)
    _confirm(client, engine, "queue::on-message", observer_info("sub-1", "t"), observer_info("sub-2", "t"))
    client.call({"function_id": "queue::on-message", "payload": {}, "metadata": {"subscriber": "sub-2"}})
    engine.deliver(invoke("queue::on-message", {}, "r-gone", metadata={"subscriber": "gone"}))
    [answer] = engine.wait_for(lambda: answers(engine, "r-gone"), what="the answer")
    assert answer["result"] == "unrouted" and "observed" not in answer
    assert seen == [["sub-2"], []]


def test_a_hook_rejection_or_call_error_keeps_its_code(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)

    @client.on_observer_deliver("queue::on-message")
    async def ack(call: Call[Any, Any], observers: Any) -> Any:
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


def test_a_handler_error_reaches_the_hook_as_an_error_call(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)

    def fail(data: Any) -> None:
        raise ValueError("out of stock")

    @client.on_observer_deliver("fail::fn")
    def recover(call: Call[Any, Any], observers: Any) -> Any:
        assert call.status == "error" and call.output is None
        assert call.error is not None and call.error["message"] == "out of stock"
        return {"recovered": [o.id for o in observers if o.matches(call)]}

    client.register_function("fail::fn", fail)
    _confirm(client, engine, "fail::fn", observer_info("e", "t", on="error"), observer_info("ok", "t"))
    assert client.call({"function_id": "fail::fn", "payload": {}}) == {"recovered": ["e"]}
    engine.deliver(invoke("fail::fn", {}, "r-fail"))
    [answer] = engine.wait_for(lambda: answers(engine, "r-fail"), what="the answer")
    assert answer["result"] == {"recovered": ["e"]} and "observed" not in answer and "error" not in answer


def test_the_deliver_hook_runs_in_the_functions_invocation(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    seen: list[Any] = []

    @client.on_observer_deliver("ctx::fn")
    def inside(call: Call[Any, Any], observers: Any) -> Any:
        seen.append(client.current_invocation())
        client.call({"function_id": "elsewhere::fn", "payload": {}, "action": CallAction.Void()})
        return call.output

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


def test_a_deliver_hook_must_take_the_call_and_the_observers(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    client.on_observer_deliver("arity::fn", lambda call, observers, extra: call.output)  # type: ignore[arg-type,misc]
    client.register_function("arity::fn", lambda d: d)
    _confirm(client, engine, "arity::fn")
    with pytest.raises(InvocationError) as raised:
        client.call({"function_id": "arity::fn", "payload": 1})
    assert raised.value.code == "invocation_failed" and isinstance(raised.value.__cause__, TypeError)


def test_a_routed_call_with_no_observers_and_no_hook_sends_only_its_answer(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    client.register_function("quiet::fn", lambda d: d)
    _confirm(client, engine, "quiet::fn")
    engine.clear()
    engine.deliver(invoke("quiet::fn", 7, "r-quiet"))
    [answer] = engine.wait_for(lambda: answers(engine, "r-quiet"), what="the answer")
    settle(client, engine)
    assert engine.frames() == [answer] and "observed" not in answer


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
    async def route(call: Call[Any, Any], observers: Any) -> Any:
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


_REF = {"channel_id": "ch-1", "access_key": "k", "direction": "read"}


@pytest.mark.parametrize("private", [True, False])
def test_the_delivery_sees_channel_refs_as_sent_and_the_handler_gets_readers(
    engine: FakeEngine, connect: Any, private: bool
) -> None:
    from iii.channels import ChannelReader

    client = connect(engine)
    bodies: list[Any] = []

    def put(data: dict[str, Any]) -> dict[str, Any]:
        bodies.append(data["body"])
        return {"stored": data["name"]}

    client.register_function("files::put", put, private=private)
    _confirm(
        client,
        engine,
        "files::put",
        observer_info("o", "t", capture=["input"], filters={"$and": [{"input.body.direction": {"$eq": "read"}}]}),
    )
    client.call({"function_id": "files::put", "payload": {"name": "a.txt", "body": _REF}})
    engine.deliver(invoke("files::put", {"name": "b.txt", "body": _REF}, "r-ref"))
    [answer] = engine.wait_for(lambda: answers(engine, "r-ref"), what="the answer")
    settle(client, engine)
    assert len(bodies) == 2 and all(isinstance(body, ChannelReader) for body in bodies)
    # filters see the ref, local or routed; the report carries it
    [item] = _items(engine, "files::put")
    assert (item["matched"], item["input"]) == (["o"], {"name": "a.txt", "body": _REF})
    assert (answer["result"], answer["observed"]) == ({"stored": "b.txt"}, ["o"])


def test_a_deliver_hook_forwards_the_call_input_with_its_channel_refs(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    forwarded: list[Any] = []

    def store(frame: dict[str, Any]) -> list[dict[str, Any]] | None:
        if frame.get("type") != "invokefunction" or frame.get("function_id") != "files::store":
            return None
        forwarded.append(frame["data"])
        return [
            {
                "type": "invocationresult",
                "invocation_id": frame["invocation_id"],
                "function_id": "files::store",
                "result": {"ok": True},
            }
        ]

    engine.on_frame = store

    @client.on_observer_deliver("files::put")
    async def forward(call: Call[Any, Any], observers: Any) -> Any:
        return await client.call_async({"function_id": "files::store", "payload": call.input})

    client.register_function("files::put", lambda data: None)
    _confirm(client, engine, "files::put")
    assert client.call({"function_id": "files::put", "payload": {"name": "a.txt", "body": _REF}}) == {"ok": True}
    engine.deliver(invoke("files::put", {"name": "b.txt", "body": _REF}, "r-forward"))
    [answer] = engine.wait_for(lambda: answers(engine, "r-forward"), what="the answer")
    assert answer["result"] == {"ok": True}
    assert forwarded == [{"name": "a.txt", "body": _REF}, {"name": "b.txt", "body": _REF}]


def test_an_event_source_reports_the_event_once_with_output_is_input(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    client.register_function("events::on-thing", lambda event: event, private=True)
    client.register_function("events::other", lambda event: dict(event), private=True)
    client.register_function("events::shared", lambda event: event)
    _confirm(client, engine, "events::on-thing", observer_info("o", "t"))
    _confirm(client, engine, "events::other", observer_info("p", "t"))
    _confirm(client, engine, "events::shared", observer_info("q", "t"))
    client.call({"function_id": "events::on-thing", "payload": {"n": 1}})
    client.call({"function_id": "events::other", "payload": {"n": 2}})
    client.call({"function_id": "events::shared", "payload": {"n": 4}})

    async def from_a_loop() -> None:
        await client.call_async({"function_id": "events::on-thing", "payload": {"n": 3}})

    asyncio.run(from_a_loop())
    settle(client, engine)
    sources = _items(engine, "events::on-thing")
    assert [s["input"] for s in sources] == [{"n": 1}, {"n": 3}]
    assert all(s["output_is_input"] is True and "output" not in s for s in sources)
    [other] = _items(engine, "events::other")
    assert other["output"] == {"n": 2} and "output_is_input" not in other
    # not private: the report carries the caller's payload, and the handler returned its own copy
    [shared] = _items(engine, "events::shared")
    assert shared["input"] == shared["output"] == {"n": 4} and "output_is_input" not in shared


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
        client.on_observer_deliver("hooked::fn", lambda call, observers: call.output)
        client.register_function("hooked::fn", lambda d: d)
        client.on_observer_match("matched::fn", lambda obs, call: True)
        client.register_function("matched::fn", lambda d: d)
        settle(client, engine)
        engine.drop()
        wait_until(lambda: len(engine.connections) == 2 and client._registered_on_connection, what="the reconnect")
        settle(client, engine)
    assert sorted(f["id"] for f in engine.frames_of("registerfunction")) == ["plain::fn", "plain::fn"]
    unsupported = [r.getMessage() for r in caplog.records if "UNSUPPORTED" in r.getMessage()]
    assert sorted(m.split("'")[1] for m in unsupported) == [
        "args::fn",
        "hooked::fn",
        "matched::fn",
        "private::fn",
        "sealed::fn",
    ]
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


def test_a_reconnect_to_an_engine_without_observe_v2_releases_a_function_that_needs_it(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    ticks: list[Any] = []
    removed: list[str] = []
    client.on_observer_removed("cron::on-tick", lambda obs: removed.append(obs.id))
    client.register_function("cron::on-tick", lambda tick: ticks.append(tick), private=True)
    _confirm(client, engine, "cron::on-tick", observer_info("o", "t"))
    state = client._functions["cron::on-tick"].observe
    assert state.local_dispatch is True
    engine.features = V1_FEATURES
    engine.drop()
    wait_until(lambda: len(engine.connections) == 2 and client._registered_on_connection, what="the reconnect")
    settle(client, engine)
    assert state.local_dispatch is False and client.observers("cron::on-tick") == [] and removed == ["o"]
    assert engine.frames_of("registerfunction", 1) == []
    # the call goes through the engine, which does not host the function
    client.call({"function_id": "cron::on-tick", "payload": {"n": 1}, "action": CallAction.Void()})
    engine.wait_for(
        lambda: [f for f in engine.frames_of("invokefunction", 1) if f["function_id"] == "cron::on-tick"],
        what="the routed call",
    )
    assert ticks == []


@pytest.mark.parametrize("kind", ["deliver", "match"])
def test_a_match_or_deliver_hook_set_after_registering_on_a_v1_engine_unregisters_the_function(
    engine_factory: Any, connect: Any, caplog: pytest.LogCaptureFixture, kind: str
) -> None:
    engine = engine_factory(V1_FEATURES)
    client = connect(engine)
    ref = client.register_function("late-hook::fn", lambda d: d)
    engine.wait_for(lambda: engine.frames_of("registerfunction"), what="the plain registration")

    def set_hook(on: bool) -> None:
        if kind == "deliver":
            ref.on_observer_deliver((lambda call, observers: call.output) if on else None)
        else:
            ref.on_observer_match((lambda obs, call: True) if on else None)

    with caplog.at_level(logging.ERROR, logger="iii.iii"):
        set_hook(True)
        [frame] = engine.wait_for(lambda: engine.frames_of("unregisterfunction"), what="the unregistration")
    assert frame["id"] == "late-hook::fn"
    assert len([r for r in caplog.records if "UNSUPPORTED" in r.getMessage()]) == 1
    # removing the hook registers it again
    set_hook(False)
    engine.wait_for(lambda: len(engine.frames_of("registerfunction")) == 2, what="the registration again")


def test_an_observer_announced_for_a_withdrawn_registration_runs_no_hook_and_gets_no_ack(
    engine_factory: Any, connect: Any
) -> None:
    """An ``observeradded`` queued behind a slow added hook when the
    registration is withdrawn, one the engine sent before it handled the
    unregistration, and its answer to the withdrawn registration run no hook
    and get no ack (decision 41), also once the function is registered again."""
    engine = engine_factory(V1_FEATURES)
    client = connect(engine)
    gate = threading.Event()
    ran: list[str] = []

    def on_added(obs: Observer) -> None:
        ran.append(obs.id)
        if obs.id == "slow":
            gate.wait(5)

    client.on_observer_added("wq::fn", on_added)
    ref = client.register_function("wq::fn", lambda d: d)
    engine.wait_for(lambda: engine.frames_of("registerfunction"), what="the registration")
    engine.deliver(function_registered("wq::fn", [observer_info("slow", "t")], revision=1))
    wait_until(lambda: ran == ["slow"], what="the slow hook")
    engine.deliver(observer_added("wq::fn", 2, observer_info("queued", "t")))
    ref.on_observer_match(lambda obs, call: True)
    engine.wait_for(lambda: engine.frames_of("unregisterfunction"), what="the withdrawal")
    engine.deliver(observer_added("wq::fn", 3, observer_info("after", "t")))
    engine.deliver(function_registered("wq::fn", [observer_info("late", "t")], revision=4))
    # the client has read those frames: an invocation behind them is answered
    engine.deliver(invoke("probe::fn", None, "r-probe"))
    engine.wait_for(lambda: answers(engine, "r-probe"), what="the probe's answer")
    # registered again (the hook is removed) before the queued add runs: it stays not added
    ref.on_observer_match(None)
    engine.wait_for(lambda: len(engine.frames_of("registerfunction")) == 2, what="the registration again")
    engine.deliver(function_registered("wq::fn", [], revision=5))
    gate.set()
    settle(client, engine)
    assert ran == ["slow"]
    assert acks(engine) == []
    assert ref.is_observed() is False


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
