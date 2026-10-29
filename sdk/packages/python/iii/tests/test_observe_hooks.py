"""Observer hooks (``on_observer_added`` / ``on_observer_removed``, worker-wide
and per function; how ``on_observer_match`` and ``on_observer_deliver`` are
set) and the observer lifecycle in the worker that hosts the function, against
a fake engine."""

from __future__ import annotations

import asyncio
import logging
import threading
import time
from typing import Any

import pytest
from observe_fake_engine import (
    FakeEngine,
    acks,
    answers,
    function_registered,
    invoke,
    observer_added,
    observer_info,
    observer_removed,
    rejected,
    settle,
    wait_until,
)

from iii import MatchKeys, Observer, ObserveRejection
from iii.function_ref import FunctionRef


def _ack_ids(engine: FakeEngine) -> list[tuple[str, int, str | None]]:
    return [(a["id"], a["revision"], (a.get("error") or {}).get("code")) for a in acks(engine)]


# ── setting hooks ──────────────────────────────────────────────────────────


def test_hook_methods_work_as_decorators_and_plain_calls_and_return_the_hook(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)

    @client.on_observer_added
    def worker_added(obs: Observer) -> None: ...

    @client.on_observer_added("fn::a")
    async def fn_added(obs: Observer) -> None: ...

    def fn_removed(obs: Observer) -> None: ...

    def deliver(call: Any, observers: Any) -> Any:
        return call.output

    @client.on_observer_match("fn::a")
    def wants(obs: Observer, call: Any) -> bool:
        return True

    def by_key(obs: Observer) -> str:
        return obs.id

    def call_key(call: Any) -> str:
        return str(call.input)

    assert client.on_observer_removed("fn::a", fn_removed) is fn_removed
    assert client.on_observer_removed(fn_removed) is fn_removed
    assert client.on_observer_deliver("fn::a", deliver) is deliver
    assert client.on_observer_deliver("fn::b")(deliver) is deliver
    keys = client.on_observer_match("fn::b", observer=by_key, call=call_key)
    assert keys == MatchKeys(observer=by_key, call=call_key)
    assert client.on_observer_match("fn::c", keys) is keys
    # the decorated names are the hooks, not anything else
    assert callable(worker_added) and asyncio.iscoroutinefunction(fn_added) and callable(wants)
    assert client._worker_added is worker_added and client._added_hooks == {"fn::a": fn_added}
    assert client._removed_hooks == {"fn::a": fn_removed} and client._worker_removed is fn_removed
    assert client._deliver_hooks == {"fn::a": deliver, "fn::b": deliver}
    assert client._match_hooks == {"fn::a": wants, "fn::b": keys, "fn::c": keys}

    # None removes
    assert client.on_observer_added(None) is None
    assert client.on_observer_added("fn::a", None) is None
    assert client.on_observer_removed("fn::a", None) is None
    assert client.on_observer_deliver("fn::b", None) is None
    assert client.on_observer_match("fn::b", None) is None
    assert (client._worker_added, client._added_hooks, client._removed_hooks) == (None, {}, {})
    assert client._deliver_hooks == {"fn::a": deliver}
    assert client._match_hooks == {"fn::a": wants, "fn::c": keys}

    with pytest.raises(TypeError):
        client.on_observer_added("fn::a", "not callable")
    with pytest.raises(TypeError):
        client.on_observer_added(worker_added, worker_added)


def test_on_observer_match_takes_a_sync_predicate_or_both_key_functions(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)

    async def async_predicate(obs: Observer, call: Any) -> bool:
        return True

    for bad in (
        lambda: client.on_observer_match("fn::a", async_predicate),
        lambda: client.on_observer_match("fn::a", "not callable"),
        lambda: client.on_observer_match("fn::a", observer=lambda obs: "k"),
        lambda: client.on_observer_match("fn::a", call=lambda call: "k"),
        lambda: client.on_observer_match("fn::a", lambda obs, call: True, observer=lambda obs: "k", call=str),
        lambda: client.on_observer_match("fn::a", observer=lambda obs: "k", call=async_predicate),
        lambda: client.on_observer_match("fn::a", MatchKeys(observer="k", call=str)),  # type: ignore[arg-type]
    ):
        with pytest.raises(TypeError):
            bad()
    assert client._match_hooks == {}


def test_a_function_ref_sets_the_hooks_of_its_id(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    ref = client.register_function("ref::fn", lambda d: d)
    assert isinstance(ref, FunctionRef) and ref.id == "ref::fn"

    @ref.on_observer_added
    def added(obs: Observer) -> None: ...

    @ref.on_observer_removed
    def removed(obs: Observer) -> None: ...

    @ref.on_observer_match
    def wants(obs: Observer, call: Any) -> bool:
        return True

    def deliver(call: Any, observers: Any) -> Any:
        return call.output

    assert ref.on_observer_deliver(deliver) is deliver
    assert (client._added_hooks["ref::fn"], client._removed_hooks["ref::fn"]) == (added, removed)
    assert client._deliver_hooks["ref::fn"] is deliver
    assert client._match_hooks["ref::fn"] is wants
    keys = ref.on_observer_match(observer=lambda obs: obs.id, call=lambda call: "k")
    assert isinstance(keys, MatchKeys) and client._match_hooks["ref::fn"] is keys
    with pytest.raises(TypeError):
        ref.on_observer_match(wants, observer=lambda obs: obs.id, call=lambda call: "k")
    assert ref.on_observer_match(None) is None and "ref::fn" not in client._match_hooks
    assert ref.on_observer_added(None) is None and "ref::fn" not in client._added_hooks
    assert ref.observers() == [] and ref.is_observed() is False


def test_the_three_forms_set_the_same_hook(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    seen: list[str] = []
    client.on_observer_added("by-id::fn", lambda obs: seen.append(f"by id {obs.id}"))
    client.register_function("by-id::fn", lambda d: d)
    client.register_function("chained::fn", lambda d: d).on_observer_added(lambda obs: seen.append(f"chained {obs.id}"))
    ref = client.register_function("ref::fn", lambda d: d)
    ref.on_observer_added(lambda obs: seen.append(f"ref {obs.id}"))
    for fn, observer_id in (("by-id::fn", "a"), ("chained::fn", "b"), ("ref::fn", "c")):
        engine.deliver(function_registered(fn, [observer_info(observer_id, "t")]))
    engine.wait_for(lambda: len(acks(engine)) == 3, what="three acks")
    assert sorted(seen) == ["by id a", "chained b", "ref c"]


def test_hooks_by_id_stay_across_unregister_and_a_later_registration(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    added: list[str] = []
    client.on_observer_added("again::fn", lambda obs: added.append(obs.id))
    client.register_function("again::fn", lambda d: d).unregister()
    client.register_function("again::fn", lambda d: d)
    engine.deliver(function_registered("again::fn", [observer_info("o", "t")]))
    engine.wait_for(lambda: acks(engine), what="the ack")
    assert added == ["o"]


def test_hooks_of_one_id_run_one_at_a_time_across_unregister_and_a_later_registration(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    log: list[str] = []

    async def on_removed(obs: Observer) -> None:
        log.append(f"remove start {obs.id}")
        await asyncio.sleep(0.2)
        log.append(f"remove end {obs.id}")

    client.on_observer_added("route::fn", lambda obs: log.append(f"add {obs.id}"))
    client.on_observer_removed("route::fn", on_removed)
    ref = client.register_function("route::fn", lambda d: d)
    engine.deliver(function_registered("route::fn", [observer_info("o", "t")], revision=1))
    engine.wait_for(lambda: acks(engine), what="the ack")
    ref.unregister()
    client.register_function("route::fn", lambda d: d)
    wait_until(lambda: "remove start o" in log, what="the removal")
    engine.deliver(function_registered("route::fn", [observer_info("o", "t")], revision=1))
    engine.wait_for(lambda: len(acks(engine)) == 2, what="the second ack")
    # the new registration's add waits for the old registration's removal of the same observer
    assert log == ["add o", "remove start o", "remove end o", "add o"]


# ── accepted: hook order, then the ack ─────────────────────────────────────


def test_on_observer_added_runs_once_per_add_and_the_observer_is_active_before_the_ack(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    seen: list[tuple[str, bool]] = []

    @client.on_observer_added("orders::create")
    async def on_added(obs: Observer) -> None:
        # inside the hook: not acked yet, not active yet
        seen.append((obs.id, client.is_observed("orders::create")))

    ref = client.register_function("orders::create", lambda data: data)
    engine.deliver(function_registered("orders::create", [observer_info("o1", "t::a")], revision=3))
    engine.wait_for(lambda: acks(engine), what="the ack")
    assert _ack_ids(engine) == [("o1", 3, None)]
    assert acks(engine)[0]["namespace"] == "default"
    assert seen == [("o1", False)]
    assert ref.is_observed() is True
    [obs] = ref.observers()
    assert (obs.id, obs.function_id, obs.target, obs.capture, obs.on) == (
        "o1",
        "orders::create",
        "t::a",
        ["input", "output", "caller"],
        "success",
    )

    engine.deliver(
        observer_added("orders::create", 4, observer_info("o2", "t::b", filters={"$and": [{"input.a": {"$eq": 1}}]}))
    )
    engine.wait_for(lambda: len(acks(engine)) == 2, what="the second ack")
    assert _ack_ids(engine)[1] == ("o2", 4, None)
    assert [o.id for o in client.observers("orders::create")] == ["o1", "o2"]
    assert ref.observers()[1].filters == {"$and": [{"input.a": {"$eq": 1}}]}
    assert seen == [("o1", False), ("o2", True)]


def test_the_workers_hook_runs_before_the_functions_and_removal_runs_in_reverse(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    log: list[str] = []
    client.on_observer_added(lambda obs: log.append(f"worker added {obs.function_id}"))
    client.on_observer_removed(lambda obs: log.append(f"worker removed {obs.function_id}"))
    client.on_observer_added("both::fn", lambda obs: log.append("function added"))
    client.on_observer_removed("both::fn", lambda obs: log.append("function removed"))
    client.register_function("both::fn", lambda d: d)
    client.register_function("worker-only::fn", lambda d: d)
    engine.deliver(function_registered("both::fn", [observer_info("o", "t")], revision=1))
    engine.wait_for(lambda: acks(engine), what="the ack")
    assert log == ["worker added both::fn", "function added"]
    engine.deliver(observer_removed("both::fn", 2, "o"))
    wait_until(lambda: len(log) == 4, what="the removal")
    assert log[2:] == ["function removed", "worker removed both::fn"]

    log.clear()
    engine.deliver(function_registered("worker-only::fn", [observer_info("w", "t")], revision=1))
    engine.wait_for(lambda: len(acks(engine)) == 2, what="the ack")
    engine.deliver(observer_removed("worker-only::fn", 2, "w"))
    wait_until(lambda: len(log) == 2, what="the removal")
    assert log == ["worker added worker-only::fn", "worker removed worker-only::fn"]


def test_a_sync_hook_runs_on_a_thread_of_its_own(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    threads: list[str] = []
    client.on_observer_added("sync::hooked", lambda obs: threads.append(threading.current_thread().name))
    client.register_function("sync::hooked", lambda d: d)
    engine.deliver(function_registered("sync::hooked", [observer_info("o1", "t")]))
    engine.wait_for(lambda: acks(engine), what="the ack")
    assert threads and threads[0] != client._thread.name


# ── rejected: the code, and unwinding ──────────────────────────────────────


@pytest.mark.parametrize(
    ("error", "code", "message"),
    [
        (ObserveRejection("INVALID_CRON_EXPRESSION", "bad day-of-week"), "INVALID_CRON_EXPRESSION", "bad day-of-week"),
        (RuntimeError("plain failure"), "OBSERVE_REJECTED", "plain failure"),
    ],
)
def test_a_raising_hook_rejects_with_its_code(
    engine: FakeEngine, connect: Any, error: Exception, code: str, message: str
) -> None:
    client = connect(engine)

    @client.on_observer_added("cron::on-tick")
    def on_added(obs: Observer) -> None:
        raise error

    ref = client.register_function("cron::on-tick", lambda d: d, private=True)
    engine.deliver(function_registered("cron::on-tick"))
    engine.deliver(observer_added("cron::on-tick", 1, observer_info("o1", "t")))
    engine.wait_for(lambda: acks(engine), what="the rejection")
    assert acks(engine)[0]["error"] == {"code": code, "message": message}
    assert ref.is_observed() is False


def test_a_function_hook_that_raises_unwinds_the_workers_hook(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    log: list[str] = []
    client.on_observer_added(lambda obs: log.append("worker added"))
    client.on_observer_removed(lambda obs: log.append("worker removed"))

    def refuse(obs: Observer) -> None:
        log.append("function added")
        raise ObserveRejection("UNKNOWN_DB", "no database 'x'")

    client.on_observer_added("unwind::fn", refuse)
    client.on_observer_removed("unwind::fn", lambda obs: log.append("function removed"))
    client.register_function("unwind::fn", lambda d: d)
    engine.deliver(function_registered("unwind::fn", [observer_info("o", "t")]))
    engine.wait_for(lambda: acks(engine), what="the rejection")
    wait_until(lambda: len(log) == 3, what="the unwinding")
    # the worker's setup is released; the function's removal never runs for an observer it did not set up
    assert log == ["worker added", "function added", "worker removed"]
    assert acks(engine)[0]["error"]["code"] == "UNKNOWN_DB"
    settle(client, engine)
    assert log == ["worker added", "function added", "worker removed"]


def test_a_workers_hook_that_raises_runs_no_other_hook(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    log: list[str] = []

    def refuse(obs: Observer) -> None:
        log.append("worker added")
        raise RuntimeError("not now")

    client.on_observer_added(refuse)
    client.on_observer_removed(lambda obs: log.append("worker removed"))
    client.on_observer_added("first::fn", lambda obs: log.append("function added"))
    client.on_observer_removed("first::fn", lambda obs: log.append("function removed"))
    client.register_function("first::fn", lambda d: d)
    engine.deliver(function_registered("first::fn", [observer_info("o", "t")]))
    engine.wait_for(lambda: acks(engine), what="the rejection")
    settle(client, engine)
    assert log == ["worker added"]
    assert acks(engine)[0]["error"] == {"code": "OBSERVE_REJECTED", "message": "not now"}


def test_a_hook_still_running_after_the_timeout_is_owner_timeout_and_a_late_success_is_undone(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    client._observe_hook_timeout_s = 0.1
    removed: list[str] = []

    async def slow_add(obs: Observer) -> None:
        await asyncio.sleep(0.3)

    client.on_observer_added("slow::fn", slow_add)
    client.on_observer_removed("slow::fn", lambda obs: removed.append(obs.id))
    ref = client.register_function("slow::fn", lambda d: d)
    engine.deliver(function_registered("slow::fn", [observer_info("o1", "t")]))
    engine.wait_for(lambda: acks(engine), what="the timeout")
    assert acks(engine)[0]["error"]["code"] == "OWNER_TIMEOUT"
    wait_until(lambda: removed == ["o1"], what="the late success to be undone")
    assert ref.is_observed() is False


def test_a_rejection_made_while_on_observer_added_runs_wins(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    log: list[str] = []

    @client.on_observer_added("reject::fn")
    def on_added(obs: Observer) -> None:
        obs.reject("ROUTE_CONFLICT", "found a conflict while indexing")

    client.on_observer_removed("reject::fn", lambda obs: log.append(f"removed {obs.id}"))
    ref = client.register_function("reject::fn", lambda d: d)
    engine.deliver(function_registered("reject::fn", [observer_info("o", "t")], revision=2))
    engine.wait_for(lambda: acks(engine), what="the ack")
    assert acks(engine)[0]["error"] == {"code": "ROUTE_CONFLICT", "message": "found a conflict while indexing"}
    assert acks(engine)[0]["revision"] == 2
    wait_until(lambda: log == ["removed o"], what="its setup to be released")
    assert ref.is_observed() is False
    # the engine removes it later: its removal hook already ran
    engine.deliver(observer_removed("reject::fn", 3, "o"))
    settle(client, engine)
    assert log == ["removed o"]


# ── removed: once per accepted observer ────────────────────────────────────


def test_on_observer_removed_runs_once_per_accepted_observer_whatever_removes_it(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    removed: list[str] = []
    client.on_observer_removed("once::fn", lambda obs: removed.append(obs.id))

    @client.on_observer_added("once::fn")
    def refuse_some(obs: Observer) -> None:
        if obs.args.get("refuse"):
            raise ObserveRejection("NO", "refused")

    ref = client.register_function("once::fn", lambda d: d)
    engine.deliver(
        function_registered(
            "once::fn",
            [
                observer_info("unobserved", "t"),
                observer_info("late", "t"),
                observer_info("refused", "t", args={"refuse": True}),
                observer_info("unregistered", "t"),
            ],
            revision=1,
        )
    )
    engine.wait_for(lambda: len(acks(engine)) == 4, what="four acks")
    # unobserve
    engine.deliver(observer_removed("once::fn", 2, "unobserved"))
    # late rejection, then the engine's removal
    [late] = [o for o in ref.observers() if o.id == "late"]
    late.reject("GONE", "config reload")
    engine.deliver(observer_removed("once::fn", 3, "late"))
    engine.deliver(observer_removed("once::fn", 4, "refused"))
    wait_until(lambda: len(removed) == 2, what="two removals")
    # disposal
    ref.unregister()
    wait_until(lambda: len(removed) == 3, what="the disposal")
    settle(client, engine)
    assert sorted(removed) == ["late", "unobserved", "unregistered"]  # never "refused"


def test_unregister_removes_the_observers_through_on_observer_removed(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    removed: list[str] = []
    client.on_observer_removed("gone::fn", lambda obs: removed.append(obs.id))
    ref = client.register_function("gone::fn", lambda d: d)
    engine.deliver(function_registered("gone::fn", [observer_info("a", "t"), observer_info("b", "t")]))
    engine.wait_for(lambda: len(acks(engine)) == 2, what="two acks")
    ref.unregister()
    wait_until(lambda: sorted(removed) == ["a", "b"], what="on_observer_removed for both")
    assert ref.is_observed() is False
    engine.wait_for(lambda: engine.frames_of("unregisterfunction"), what="unregisterfunction")


def test_an_on_observer_removed_error_is_logged_and_the_rest_goes_on(
    engine: FakeEngine, connect: Any, caplog: pytest.LogCaptureFixture
) -> None:
    client = connect(engine)
    removed: list[str] = []

    def on_removed(obs: Observer) -> None:
        removed.append(obs.id)
        if obs.id == "a":
            raise RuntimeError("cannot release")

    client.on_observer_removed("err::fn", on_removed)
    client.on_observer_removed(lambda obs: removed.append(f"worker {obs.id}"))
    ref = client.register_function("err::fn", lambda d: d)
    engine.deliver(function_registered("err::fn", [observer_info("a", "t"), observer_info("b", "t")]))
    engine.wait_for(lambda: len(acks(engine)) == 2, what="two acks")
    with caplog.at_level(logging.ERROR, logger="iii.observe"):
        engine.deliver(observer_removed("err::fn", 1, "a"))
        engine.deliver(observer_removed("err::fn", 2, "b"))
        wait_until(lambda: len(removed) == 4, what="both removals")
    assert "cannot release" in caplog.text
    assert removed == ["a", "worker a", "b", "worker b"]  # the worker's still runs
    assert ref.is_observed() is False


# ── one at a time per function ─────────────────────────────────────────────


def test_hooks_of_one_function_run_one_at_a_time_in_announcement_order(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    log: list[str] = []

    async def worker_added(obs: Observer) -> None:
        log.append(f"worker {obs.id}")
        await asyncio.sleep(0.02)

    async def on_added(obs: Observer) -> None:
        log.append(f"start {obs.id}")
        await asyncio.sleep(0.05 if obs.id == "a" else 0)
        log.append(f"end {obs.id}")

    async def on_removed(obs: Observer) -> None:
        log.append(f"removed {obs.id}")

    client.on_observer_added(worker_added)
    client.on_observer_added("serial::fn", on_added)
    client.on_observer_removed("serial::fn", on_removed)
    client.register_function("serial::fn", lambda d: d)
    engine.deliver(function_registered("serial::fn"))
    engine.deliver(observer_added("serial::fn", 1, observer_info("a", "t")))
    engine.deliver(observer_added("serial::fn", 2, observer_info("b", "t")))
    engine.deliver(observer_removed("serial::fn", 3, "a"))
    engine.wait_for(lambda: len(acks(engine)) == 2, what="two acks")
    wait_until(lambda: len(log) == 7, what="the hooks")
    assert log == ["worker a", "start a", "end a", "worker b", "start b", "end b", "removed a"]
    assert [a["revision"] for a in acks(engine)] == [1, 2]


def test_hooks_of_different_functions_run_concurrently(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    gate = threading.Event()
    log: list[str] = []

    async def blocked(obs: Observer) -> None:
        log.append("blocked started")
        while not gate.is_set():
            await asyncio.sleep(0.01)

    async def free(obs: Observer) -> None:
        log.append("free ran")
        gate.set()

    client.on_observer_added("one::fn", blocked)
    client.on_observer_added("two::fn", free)
    client.register_function("one::fn", lambda d: d)
    client.register_function("two::fn", lambda d: d)
    engine.deliver(function_registered("one::fn", [observer_info("a", "t")]))
    engine.deliver(function_registered("two::fn", [observer_info("b", "t")]))
    engine.wait_for(lambda: len(acks(engine)) == 2, what="both acks")
    assert log == ["blocked started", "free ran"]


# ── catch-up and warnings ──────────────────────────────────────────────────


def test_a_hook_set_once_observers_are_active_is_caught_up_with_one_warning(
    engine: FakeEngine, connect: Any, caplog: pytest.LogCaptureFixture
) -> None:
    client = connect(engine)
    ref = client.register_function("late::fn", lambda d: d)
    engine.deliver(
        function_registered(
            "late::fn", [observer_info("a", "t"), observer_info("b", "t", args={"bad": True})], revision=1
        )
    )
    engine.wait_for(lambda: len(acks(engine)) == 2, what="two acks")
    engine.clear()
    started: list[str] = []

    with caplog.at_level(logging.WARNING, logger="iii.observe"):

        @ref.on_observer_added
        def start(obs: Observer) -> None:
            started.append(obs.id)
            if obs.args.get("bad"):
                raise ObserveRejection("BAD_ARGS", "cannot start")

        engine.wait_for(lambda: acks(engine), what="the late rejection")
    assert started == ["a", "b"]
    warnings = [r.getMessage() for r in caplog.records if "on_observer_added was set for" in r.getMessage()]
    assert len(warnings) == 1 and "late::fn" in warnings[0] and " 2 " in warnings[0]
    assert _ack_ids(engine) == [("b", 1, "BAD_ARGS")]  # a throw late-rejects that observer
    assert [o.id for o in ref.observers()] == ["a"]
    # set again: nothing is behind, nothing runs, nothing is logged
    caplog.clear()
    ref.on_observer_added(start)
    settle(client, engine)
    assert started == ["a", "b"]
    assert not [r for r in caplog.records if "on_observer_added was set for" in r.getMessage()]


def test_a_workers_hook_set_late_catches_up_every_function(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    client.register_function("x::fn", lambda d: d)
    client.register_function("y::fn", lambda d: d)
    engine.deliver(function_registered("x::fn", [observer_info("x1", "t")]))
    engine.deliver(function_registered("y::fn", [observer_info("y1", "t"), observer_info("y2", "t")]))
    engine.wait_for(lambda: len(acks(engine)) == 3, what="three acks")
    retained: list[str] = []
    client.on_observer_added(lambda obs: retained.append(obs.id))
    wait_until(lambda: len(retained) == 3, what="the catch-up")
    assert sorted(retained) == ["x1", "y1", "y2"]


def _unregistered_warnings(caplog: pytest.LogCaptureFixture) -> list[str]:
    return [r.getMessage() for r in caplog.records if "has not registered" in r.getMessage()]


def test_hook_ids_still_unregistered_after_the_grace_period_are_warned_once(
    engine: FakeEngine, connect: Any, caplog: pytest.LogCaptureFixture
) -> None:
    client = connect(engine)
    client._hook_id_check_delay_s = 0.05
    with caplog.at_level(logging.WARNING, logger="iii.iii"):
        # set after the connection is registered: checked once the grace period passes
        client.on_observer_added("typo::fn", lambda obs: None)
        client.on_observer_match("typo::match", lambda obs, call: True)
        client.on_observer_deliver("orders::create", lambda call, observers: call.output)
        client.register_function("orders::create", lambda d: d)
        wait_until(lambda: len(_unregistered_warnings(caplog)) == 2, what="the warnings")
        engine.drop()
        wait_until(lambda: len(engine.connections) == 2 and client._registered_on_connection, what="the reconnect")
        time.sleep(0.15)  # past the next check
    warned = sorted(_unregistered_warnings(caplog))
    assert len(warned) == 2 and "typo::fn" in warned[0] and "typo::match" in warned[1]


def test_a_hook_set_by_id_before_its_registration_is_not_warned_when_workerregistered_lands_between(
    engine: FakeEngine, connect: Any, caplog: pytest.LogCaptureFixture
) -> None:
    client = connect(engine)
    client._hook_id_check_delay_s = 0.2
    with caplog.at_level(logging.WARNING, logger="iii.iii"):
        client.on_observer_added("later::fn", lambda obs: None)
        engine.deliver({"type": "workerregistered", "worker_id": engine.worker_id, "features": engine.features})
        settle(client, engine)
        client.register_function("later::fn", lambda d: d)
        time.sleep(0.4)  # past the check
    assert _unregistered_warnings(caplog) == []


# ── the engine's set: diff, revisions ──────────────────────────────────────


def test_added_and_removed_messages_with_a_stale_revision_are_ignored(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    added: list[str] = []
    client.on_observer_added("rev::fn", lambda obs: added.append(obs.id))
    client.register_function("rev::fn", lambda d: d)
    engine.deliver(function_registered("rev::fn", revision=5))
    engine.deliver(observer_added("rev::fn", 5, observer_info("stale", "t")))
    engine.deliver(observer_added("rev::fn", 6, observer_info("fresh", "t")))
    engine.deliver(observer_removed("rev::fn", 6, "fresh"))
    engine.wait_for(lambda: acks(engine), what="the ack")
    settle(client, engine)
    assert added == ["fresh"]
    assert _ack_ids(engine) == [("fresh", 6, None)]


def test_a_fresh_functionregistered_is_diffed_removed_added_kept(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    added: list[str] = []
    removed: list[str] = []
    client.on_observer_added("diff::fn", lambda obs: added.append(obs.id))
    client.on_observer_removed("diff::fn", lambda obs: removed.append(obs.id))
    ref = client.register_function("diff::fn", lambda d: d)
    engine.deliver(
        function_registered("diff::fn", [observer_info("keep", "t"), observer_info("gone", "t")], revision=2)
    )
    engine.wait_for(lambda: len(acks(engine)) == 2, what="two acks")
    kept = ref.observers()[0]
    engine.clear()

    # e.g. after a reconnect: the engine's full set replaces the SDK's
    engine.deliver(
        function_registered(
            "diff::fn",
            [observer_info("keep", "t", filters={"$and": [{"input.n": {"$gt": 1}}]}), observer_info("new", "t")],
            revision=7,
        )
    )
    engine.wait_for(lambda: len(acks(engine)) == 2, what="the re-acks")
    assert removed == ["gone"]
    assert added == ["keep", "gone", "new"]  # `keep` is acked again without its hook
    assert sorted(_ack_ids(engine)) == [("keep", 7, None), ("new", 7, None)]
    assert ref.observers()[0] is kept  # the same object, with the engine's current filters
    assert kept.filters == {"$and": [{"input.n": {"$gt": 1}}]}
    assert kept.matches({"input": {"n": 2}}) is True
    assert kept.matches({"input": {"n": 1}}) is False


def test_changed_args_are_a_different_observer(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    log: list[str] = []
    client.on_observer_added("args::fn", lambda obs: log.append(f"added {obs.args}"))
    client.on_observer_removed("args::fn", lambda obs: log.append(f"removed {obs.args}"))
    client.register_function("args::fn", lambda d: d, args_format={"type": "object"})
    engine.deliver(function_registered("args::fn", [observer_info("o", "t", args={"x": 1})]))
    engine.wait_for(lambda: acks(engine), what="the ack")
    engine.deliver(function_registered("args::fn", [observer_info("o", "t", args={"x": 2})], revision=1))
    engine.wait_for(lambda: len(acks(engine)) == 2, what="the re-ack")
    assert log == ["added {'x': 1}", "removed {'x': 1}", "added {'x': 2}"]


def test_an_observer_announced_again_with_the_same_args_runs_no_hook_and_is_served_with_that_view(
    engine: FakeEngine, connect: Any
) -> None:
    """Decision 44: a held observer announced again with the same args is
    re-acked without any hook, and served with the filters, target and
    capture it was announced with; listed by ``functionregistered`` or by
    ``observeradded``. Other args are a removal and then an add."""
    client = connect(engine)
    log: list[str] = []
    client.on_observer_added("ra::fn", lambda obs: log.append(f"added {obs.id} {obs.args}"))
    client.on_observer_removed("ra::fn", lambda obs: log.append(f"removed {obs.id} {obs.args}"))
    ref = client.register_function("ra::fn", lambda data: data)
    engine.deliver(function_registered("ra::fn", [observer_info("a", "t::a", args={"kind": "a"})], revision=1))
    engine.wait_for(lambda: acks(engine), what="the ack")
    [held] = ref.observers()

    def above(n: int) -> dict[str, Any]:
        return observer_info(
            "a", "t::target", args={"kind": "a"}, filters={"$and": [{"input.n": {"$gt": n}}]}, capture=["input"]
        )

    calls = iter(range(1, 100))

    def observed(n: int) -> list[str] | None:
        invocation_id = f"r-{next(calls)}"
        engine.deliver(invoke("ra::fn", {"n": n}, invocation_id))
        [answer] = engine.wait_for(lambda: answers(engine, invocation_id), what="the answer")
        return answer.get("observed")

    # listed by functionregistered
    engine.deliver(function_registered("ra::fn", [above(1)], revision=2))
    engine.wait_for(lambda: len(acks(engine)) == 2, what="the re-ack")
    assert observed(1) is None
    assert observed(2) == ["a"]
    # announced by observeradded
    engine.deliver(observer_added("ra::fn", 3, above(5)))
    engine.wait_for(lambda: len(acks(engine)) == 3, what="the second re-ack")
    assert observed(2) is None
    assert observed(6) == ["a"]
    assert log == ["added a {'kind': 'a'}"]
    assert _ack_ids(engine) == [("a", 1, None), ("a", 2, None), ("a", 3, None)]
    assert ref.observers() == [held]
    assert (held.target, held.capture, held.filters) == ("t::target", ["input"], above(5)["filters"])

    # its late rejection carries the revision announced last; listed again, whatever its args, it is re-acked with it
    held.reject("GONE", "config reloaded")
    engine.deliver(function_registered("ra::fn", [observer_info("a", "t::a", args={"kind": "other"})], revision=4))
    engine.wait_for(lambda: len(acks(engine)) == 5, what="the rejection, twice")
    assert _ack_ids(engine)[3:] == [("a", 3, "GONE"), ("a", 4, "GONE")]
    assert ref.is_observed() is False

    # a fresh observeradded brings it back as a new observer; other args: removed, then added
    engine.deliver(observer_added("ra::fn", 5, observer_info("a", "t::a", args={"kind": "a"})))
    engine.wait_for(lambda: len(acks(engine)) == 6, what="the fresh ack")
    engine.deliver(observer_added("ra::fn", 6, observer_info("a", "t::a", args={"kind": "b"})))
    engine.wait_for(lambda: len(acks(engine)) == 7, what="the ack of the new args")
    settle(client, engine)
    assert log == [
        "added a {'kind': 'a'}",
        "removed a {'kind': 'a'}",
        "added a {'kind': 'a'}",
        "removed a {'kind': 'a'}",
        "added a {'kind': 'b'}",
    ]
    assert _ack_ids(engine)[5:] == [("a", 5, None), ("a", 6, None)]
    assert [o.args for o in ref.observers()] == [{"kind": "b"}]


def test_a_held_observer_announced_with_filters_that_do_not_compile_is_removed_then_refused(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    removed: list[str] = []
    client.on_observer_removed("bad-again::fn", lambda obs: removed.append(obs.id))
    ref = client.register_function("bad-again::fn", lambda d: d)
    engine.deliver(function_registered("bad-again::fn", [observer_info("o", "t")], revision=3))
    engine.wait_for(lambda: acks(engine), what="the ack")
    bad = observer_info("o", "t")
    bad["filters"] = ["not", "an", "object"]
    engine.deliver(function_registered("bad-again::fn", [bad], revision=5))
    engine.wait_for(lambda: len(acks(engine)) == 2, what="the refusal")
    settle(client, engine)
    assert removed == ["o"]
    assert _ack_ids(engine) == [("o", 3, None), ("o", 5, "INVALID_FILTER")]
    assert ref.observers() == []


def test_a_held_observer_served_a_changed_view_is_filed_under_its_new_keys(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    added: list[str] = []
    client.on_observer_added("rk::fn", lambda obs: added.append(obs.id))
    client.on_observer_match("rk::fn", observer=lambda obs: obs.target, call=lambda call: call.input["to"])
    ref = client.register_function("rk::fn", lambda d: d)
    engine.deliver(function_registered("rk::fn", [observer_info("a", "t::one")], revision=1))
    engine.wait_for(lambda: acks(engine), what="the ack")
    assert ref.is_observed({"input": {"to": "t::one"}}) is True
    engine.deliver(observer_added("rk::fn", 2, observer_info("a", "t::two")))
    engine.wait_for(lambda: len(acks(engine)) == 2, what="the re-ack")
    assert added == ["a"]
    assert ref.is_observed({"input": {"to": "t::one"}}) is False
    assert ref.is_observed({"input": {"to": "t::two"}}) is True


def test_a_malformed_filter_from_the_engine_is_refused(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    client.register_function("bad::fn", lambda d: d)
    bad = observer_info("o", "t")
    bad["filters"] = ["not", "an", "object"]
    engine.deliver(function_registered("bad::fn", [bad, {**observer_info("ok", "t"), "filters": None}]))
    engine.wait_for(lambda: len(acks(engine)) == 2, what="both acks")
    by_id = {a["id"]: a for a in acks(engine)}
    assert by_id["o"]["error"]["code"] == "INVALID_FILTER"
    assert "error" not in by_id["ok"]  # null is "absent": every call


def test_a_late_rejection_is_re_acked_with_the_announced_revision_until_a_fresh_add(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    ref = client.register_function("late-reject::fn", lambda d: d)
    engine.deliver(function_registered("late-reject::fn", [observer_info("o", "t")], revision=4))
    engine.wait_for(lambda: acks(engine), what="the ack")
    obs = ref.observers()[0]
    engine.clear()
    obs.reject("UNKNOWN_DB", "the database was removed by a config reload")
    engine.wait_for(lambda: acks(engine), what="the late rejection")
    assert acks(engine)[0] == {
        "type": "observerack",
        "function_id": "late-reject::fn",
        "namespace": "default",
        "id": "o",
        "revision": 4,
        "error": {"code": "UNKNOWN_DB", "message": "the database was removed by a config reload"},
    }
    assert ref.is_observed() is False

    # a later functionregistered still listing it: re-acked with the rejection, not reactivated
    engine.clear()
    engine.deliver(function_registered("late-reject::fn", [observer_info("o", "t")], revision=9))
    engine.wait_for(lambda: acks(engine), what="the re-ack")
    assert (acks(engine)[0]["revision"], acks(engine)[0]["error"]["code"]) == (9, "UNKNOWN_DB")
    assert ref.is_observed() is False

    # a fresh observeradded starts it over
    engine.clear()
    engine.deliver(observer_added("late-reject::fn", 10, observer_info("o", "t")))
    engine.wait_for(lambda: acks(engine), what="the fresh ack")
    assert "error" not in acks(engine)[0]
    assert ref.is_observed() is True


# ── releases: namespace conflict, unregister ───────────────────────────────


def test_function_namespace_conflict_clears_local_dispatch_and_the_observer_set(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    removed: list[str] = []
    client.on_observer_removed("dup::fn", lambda obs: removed.append(obs.id))
    ref = client.register_function("dup::fn", lambda d: d)
    engine.deliver(function_registered("dup::fn", [observer_info("o1", "t")]))
    engine.wait_for(lambda: acks(engine), what="the ack")
    assert client._functions["dup::fn"].observe.local_dispatch is True
    engine.deliver(rejected("dup::fn"))
    wait_until(lambda: removed == ["o1"], what="on_observer_removed")
    assert ref.is_observed() is False
    assert client._functions["dup::fn"].observe.local_dispatch is False
    assert client.get_connection_state() == "connected"


def test_reports_held_for_a_conflicted_function_are_released_as_inactive(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    client.register_function("held::fn", lambda d: d)
    engine.deliver(function_registered("held::fn", [observer_info("o", "t", capture=["input"])]))
    engine.wait_for(lambda: acks(engine), what="the ack")
    state = client._functions["held::fn"].observe
    # a report waiting for the function to be confirmed on the current connection
    state.confirmed_epoch = -1
    client.call({"function_id": "held::fn", "payload": 1})
    wait_until(lambda: client._reporter.size == 1, what="the held report")
    engine.deliver(rejected("held::fn"))
    wait_until(lambda: client._reporter.size == 0, what="the release")

    async def stats() -> bool:
        return client._reporter.send_stats()

    assert client._run_on_loop(stats()) is True
    [frame] = engine.wait_for(lambda: engine.frames_of("localinvocationstats"), what="the stats")
    assert frame["dropped_reports"] == [{"function_id": "held::fn", "reason": "INACTIVE", "count": 1}]
    assert engine.frames_of("invocationsobserved") == []


def test_a_namespace_conflict_while_a_hook_runs_undoes_that_add(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    entered = threading.Event()
    release = threading.Event()
    log: list[str] = []

    def on_added(obs: Observer) -> None:
        log.append(f"added {obs.id}")
        if obs.id == "slow":
            entered.set()
            release.wait(5)
            log.append("slow hook returned")

    client.on_observer_added("conflict::fn", on_added)
    client.on_observer_removed("conflict::fn", lambda obs: log.append(f"removed {obs.id}"))
    ref = client.register_function("conflict::fn", lambda d: d)
    engine.deliver(function_registered("conflict::fn", [observer_info("fast", "t")]))
    engine.wait_for(lambda: acks(engine), what="the first ack")
    engine.deliver(observer_added("conflict::fn", 1, observer_info("slow", "t")))
    assert entered.wait(5)
    engine.deliver(rejected("conflict::fn"))
    wait_until(lambda: not client._functions["conflict::fn"].observe.local_dispatch, what="the release")
    release.set()
    wait_until(lambda: len(log) == 5, what="the hooks")
    # the drop ran after the queued add, which was undone instead of acked
    assert log == ["added fast", "added slow", "slow hook returned", "removed slow", "removed fast"]
    settle(client, engine)
    assert [a["id"] for a in acks(engine)] == ["fast"]
    assert ref.is_observed() is False


def test_an_add_queued_behind_a_running_hook_never_runs_after_a_namespace_conflict(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    entered = threading.Event()
    release = threading.Event()
    log: list[str] = []

    def on_added(obs: Observer) -> None:
        log.append(f"added {obs.id}")
        if obs.id == "slow":
            entered.set()
            release.wait(5)
            log.append("slow hook returned")

    client.on_observer_added("queued::fn", on_added)
    client.on_observer_removed("queued::fn", lambda obs: log.append(f"removed {obs.id}"))
    ref = client.register_function("queued::fn", lambda d: d)
    engine.deliver(function_registered("queued::fn"))
    engine.deliver(observer_added("queued::fn", 1, observer_info("slow", "t")))
    assert entered.wait(5)
    engine.deliver(observer_added("queued::fn", 2, observer_info("queued", "t")))  # waits behind "slow"
    engine.deliver(rejected("queued::fn"))
    wait_until(lambda: not client._functions["queued::fn"].observe.local_dispatch, what="the release")
    release.set()
    settle(client, engine)
    assert log == ["added slow", "slow hook returned", "removed slow"]  # "queued" never ran its hook
    assert acks(engine) == []
    assert ref.is_observed() is False


def test_a_reject_from_another_thread_right_before_activation_wins(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    client.on_observer_added("race::fn", lambda obs: None)
    ref = client.register_function("race::fn", lambda d: d)
    state = client._functions["race::fn"].observe
    activate = state._activate

    def reject_first(obs: Observer) -> bool:
        # the worker rejects it from another thread in the window before activation
        other = threading.Thread(target=obs.reject, args=("RACE", "rejected right before activation"))
        other.start()
        other.join()
        return activate(obs)

    state._activate = reject_first
    engine.deliver(function_registered("race::fn"))
    engine.deliver(observer_added("race::fn", 1, observer_info("late", "t")))
    engine.wait_for(lambda: acks(engine), what="the ack")
    settle(client, engine)
    codes = [(a["id"], (a.get("error") or {}).get("code")) for a in acks(engine)]
    assert codes and all(code == ("late", "RACE") for code in codes)  # never a success ack
    assert ref.is_observed() is False


def test_a_fresh_re_add_after_a_late_rejection_does_not_run_once_the_function_was_released(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    removing = threading.Event()
    release = threading.Event()
    log: list[str] = []

    def on_removed(obs: Observer) -> None:
        log.append(f"removed {obs.id}")
        removing.set()
        release.wait(5)

    client.on_observer_added("readd::fn", lambda obs: log.append(f"added {obs.id}"))
    client.on_observer_removed("readd::fn", on_removed)
    ref = client.register_function("readd::fn", lambda d: d, private=True)
    engine.deliver(function_registered("readd::fn", [observer_info("o", "t")], revision=1))
    engine.wait_for(lambda: acks(engine), what="the ack")
    ref.observers()[0].reject("GONE", "rejected late")
    engine.wait_for(lambda: len(acks(engine)) == 2, what="the late rejection")
    # a fresh add of the rejected observer: its old self is removed first (the hook blocks) ...
    engine.deliver(observer_added("readd::fn", 2, observer_info("o", "t")))
    assert removing.wait(5)
    # ... and the function is released meanwhile
    engine.deliver(rejected("readd::fn"))
    wait_until(lambda: not client._functions["readd::fn"].observe.local_dispatch, what="the release")
    release.set()
    settle(client, engine)
    assert log == ["added o", "removed o"]  # the re-add never ran its hook
    assert len(acks(engine)) == 2  # no success ack after the release
    assert ref.is_observed() is False


# ── engines without observe ────────────────────────────────────────────────


def test_against_an_engine_without_observe_hooks_are_inactive_and_logged_once(
    engine_factory: Any, connect: Any, caplog: pytest.LogCaptureFixture
) -> None:
    legacy = engine_factory("legacy")
    with caplog.at_level(logging.WARNING, logger="iii.iii"):
        client = connect(legacy)
        client.on_observer_added("hooked::fn", lambda obs: None)
        client.register_function("hooked::fn", lambda d: d)
        client.register_function("plain::fn", lambda d: d)
        legacy.wait_for(
            lambda: [f for f in legacy.frames_of("registerfunction") if f["id"] == "hooked::fn"], what="registration"
        )
        legacy.drop()
        wait_until(lambda: len(legacy.connections) == 2 and client._registered_on_connection, what="the reconnect")
    inactive = [r for r in caplog.records if "hooks are inactive" in r.getMessage()]
    assert len(inactive) == 1
    assert "hooked::fn" in inactive[0].getMessage()
    assert client._functions["plain::fn"].observe.local_dispatch is False  # no functionregistered: no local dispatch
