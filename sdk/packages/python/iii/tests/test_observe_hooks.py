"""The owner's observe hooks (sdk.md §1, §4.1, §4.2) against a fake engine."""

from __future__ import annotations

import asyncio
import logging
import threading
from typing import Any

import pytest
from observe_fake_engine import (
    FakeEngine,
    acks,
    function_registered,
    observer_added,
    observer_info,
    observer_removed,
    settle,
    wait_until,
)

from iii import InvocationError, ObserveHooks, Observer, ObserveRejection


def _ack_ids(engine: FakeEngine) -> list[tuple[str, int, str | None]]:
    return [(a["id"], a["revision"], (a.get("error") or {}).get("code")) for a in acks(engine)]


def test_on_observer_added_runs_once_per_add_and_the_observer_is_active_before_the_ack(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    seen: list[tuple[str, bool]] = []
    ref_box: list[Any] = []

    async def on_added(obs: Observer) -> None:
        # inside the hook: not acked yet, not active yet
        seen.append((obs.id, ref_box[0].is_observed()))

    ref = client.register_function(
        "orders::create", lambda data: data, observe=ObserveHooks(on_observer_added=on_added)
    )
    ref_box.append(ref)
    engine.wait_for(lambda: engine.frames_of("registerfunction"), what="registerfunction")
    frame = engine.frames_of("registerfunction")[0]
    assert "observe" not in frame  # the hooks never travel

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
    assert [o.id for o in ref.observers()] == ["o1", "o2"]
    assert ref.observers()[1].filters == {"$and": [{"input.a": {"$eq": 1}}]}
    assert seen == [("o1", False), ("o2", True)]


def test_a_sync_hook_runs_on_a_thread_of_its_own(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    threads: list[str] = []

    def on_added(obs: Observer) -> None:
        threads.append(threading.current_thread().name)

    client.register_function("sync::hooked", lambda d: d, observe={"on_observer_added": on_added})
    engine.deliver(function_registered("sync::hooked", [observer_info("o1", "t")]))
    engine.wait_for(lambda: acks(engine), what="the ack")
    assert threads and threads[0] != client._thread.name


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

    def on_added(obs: Observer) -> None:
        raise error

    ref = client.register_function(
        "cron::on-tick", lambda d: None, observe=ObserveHooks(delivery="owner", on_observer_added=on_added)
    )
    engine.deliver(function_registered("cron::on-tick"))
    engine.deliver(observer_added("cron::on-tick", 1, observer_info("o1", "t")))
    engine.wait_for(lambda: acks(engine), what="the rejection")
    assert acks(engine)[0]["error"] == {"code": code, "message": message}
    assert ref.is_observed() is False


def test_a_hook_still_running_after_the_timeout_is_owner_timeout_and_a_late_success_is_undone(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    client._observe_hook_timeout_s = 0.1
    removed: list[str] = []

    async def slow_add(obs: Observer) -> None:
        await asyncio.sleep(0.3)

    ref = client.register_function(
        "slow::fn",
        lambda d: None,
        observe=ObserveHooks(on_observer_added=slow_add, on_observer_removed=lambda obs: removed.append(obs.id)),
    )
    engine.deliver(function_registered("slow::fn", [observer_info("o1", "t")]))
    engine.wait_for(lambda: acks(engine), what="the timeout")
    assert acks(engine)[0]["error"]["code"] == "OWNER_TIMEOUT"
    wait_until(lambda: removed == ["o1"], what="the late success to be undone")
    assert ref.is_observed() is False


def test_hooks_of_one_function_run_one_at_a_time_in_revision_order(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    log: list[str] = []

    async def on_added(obs: Observer) -> None:
        log.append(f"start {obs.id}")
        await asyncio.sleep(0.05 if obs.id == "a" else 0)
        log.append(f"end {obs.id}")

    async def on_removed(obs: Observer) -> None:
        log.append(f"removed {obs.id}")

    client.register_function(
        "serial::fn", lambda d: None, observe=ObserveHooks(on_observer_added=on_added, on_observer_removed=on_removed)
    )
    engine.deliver(function_registered("serial::fn"))
    engine.deliver(observer_added("serial::fn", 1, observer_info("a", "t")))
    engine.deliver(observer_added("serial::fn", 2, observer_info("b", "t")))
    engine.deliver(observer_removed("serial::fn", 3, "a"))
    engine.wait_for(lambda: len(acks(engine)) == 2, what="two acks")
    wait_until(lambda: len(log) == 5, what="the hooks")
    assert log == ["start a", "end a", "start b", "end b", "removed a"]
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

    client.register_function("one::fn", lambda d: None, observe=ObserveHooks(on_observer_added=blocked))
    client.register_function("two::fn", lambda d: None, observe=ObserveHooks(on_observer_added=free))
    engine.deliver(function_registered("one::fn", [observer_info("a", "t")]))
    engine.deliver(function_registered("two::fn", [observer_info("b", "t")]))
    engine.wait_for(lambda: len(acks(engine)) == 2, what="both acks")
    assert log == ["blocked started", "free ran"]


def test_added_and_removed_messages_with_a_stale_revision_are_ignored(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    added: list[str] = []
    client.register_function(
        "rev::fn", lambda d: None, observe=ObserveHooks(on_observer_added=lambda obs: added.append(obs.id))
    )
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
    ref = client.register_function(
        "diff::fn",
        lambda d: None,
        observe=ObserveHooks(
            on_observer_added=lambda obs: added.append(obs.id),
            on_observer_removed=lambda obs: removed.append(obs.id),
        ),
    )
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
    client.register_function(
        "args::fn",
        lambda d: None,
        observe=ObserveHooks(
            delivery="owner",
            on_observer_added=lambda obs: log.append(f"added {obs.args}"),
            on_observer_removed=lambda obs: log.append(f"removed {obs.args}"),
        ),
    )
    engine.deliver(function_registered("args::fn", [observer_info("o", "t", args={"x": 1})]))
    engine.wait_for(lambda: acks(engine), what="the ack")
    engine.deliver(function_registered("args::fn", [observer_info("o", "t", args={"x": 2})], revision=1))
    engine.wait_for(lambda: len(acks(engine)) == 2, what="the re-ack")
    assert log == ["added {'x': 1}", "removed {'x': 1}", "added {'x': 2}"]


def test_a_malformed_filter_from_the_engine_is_refused(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    client.register_function("bad::fn", lambda d: None)
    bad = observer_info("o", "t")
    bad["filters"] = ["not", "an", "object"]
    engine.deliver(function_registered("bad::fn", [bad, {**observer_info("ok", "t"), "filters": None}]))
    engine.wait_for(lambda: len(acks(engine)) == 2, what="both acks")
    by_id = {a["id"]: a for a in acks(engine)}
    assert by_id["o"]["error"]["code"] == "INVALID_FILTER"
    assert "error" not in by_id["ok"]  # null is "absent": every event


def test_function_namespace_conflict_clears_local_dispatch_and_the_observer_set(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    removed: list[str] = []
    ref = client.register_function(
        "dup::fn", lambda d: d, observe=ObserveHooks(on_observer_removed=lambda obs: removed.append(obs.id))
    )
    engine.deliver(function_registered("dup::fn", [observer_info("o1", "t")]))
    engine.wait_for(lambda: acks(engine), what="the ack")
    assert client._functions["dup::fn"].observe.local_dispatch is True
    engine.deliver(
        {
            "type": "registrationrejected",
            "code": "FUNCTION_NAMESPACE_CONFLICT",
            "namespace": "default",
            "function_id": "dup::fn",
            "owner_worker_id": "other",
        }
    )
    wait_until(lambda: removed == ["o1"], what="on_observer_removed")
    assert ref.is_observed() is False
    assert client._functions["dup::fn"].observe.local_dispatch is False
    assert client.get_connection_state() == "connected"


def test_unregister_removes_the_observers_through_on_observer_removed(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    removed: list[str] = []
    ref = client.register_function(
        "gone::fn", lambda d: d, observe=ObserveHooks(on_observer_removed=lambda obs: removed.append(obs.id))
    )
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

    ref = client.register_function("err::fn", lambda d: d, observe=ObserveHooks(on_observer_removed=on_removed))
    engine.deliver(function_registered("err::fn", [observer_info("a", "t"), observer_info("b", "t")]))
    engine.wait_for(lambda: len(acks(engine)) == 2, what="two acks")
    with caplog.at_level(logging.ERROR, logger="iii.observe"):
        engine.deliver(observer_removed("err::fn", 1, "a"))
        engine.deliver(observer_removed("err::fn", 2, "b"))
        wait_until(lambda: removed == ["a", "b"], what="both removals")
    assert "cannot release" in caplog.text
    assert ref.is_observed() is False


def test_a_shim_observer_carries_the_legacy_trigger_config(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    got: list[Observer] = []
    client.register_function(
        "http::on-request",
        lambda d: None,
        observe=ObserveHooks(delivery="owner", legacy_trigger_type="http", on_observer_added=got.append),
    )
    frame = engine.wait_for(lambda: engine.frames_of("registerfunction"), what="registerfunction")[0]
    assert frame["observe"] == {"delivery": "owner", "legacy_trigger_type": "http"}
    engine.deliver(function_registered("http::on-request"))
    info = observer_info("legacy-1", "users::get")
    info["legacy"] = {"trigger_type": "http", "config": {"api_path": "/users/:id", "http_method": "GET"}}
    engine.deliver(observer_added("http::on-request", 1, info))
    engine.wait_for(lambda: acks(engine), what="the ack")
    assert got[0].legacy == {"trigger_type": "http", "config": {"api_path": "/users/:id", "http_method": "GET"}}
    assert got[0].args == {}
    assert got[0].to_dict()["legacy"]["trigger_type"] == "http"


def test_a_subclass_declares_the_hooks_as_methods(engine: FakeEngine, connect: Any) -> None:
    index: dict[str, set[str]] = {}

    class CronHooks(ObserveHooks):
        delivery = "owner"
        args_format = {
            "type": "object",
            "additionalProperties": False,
            "required": ["expression"],
            "properties": {"expression": {"type": "string"}},
        }

        async def on_observer_added(self, obs: Observer) -> None:  # type: ignore[override]
            index.setdefault(obs.args["expression"], set()).add(obs.id)

        async def on_observer_removed(self, obs: Observer) -> None:  # type: ignore[override]
            index[obs.args["expression"]].discard(obs.id)

    client = connect(engine)
    client.register_function("cron::on-tick", lambda tick: None, observe=CronHooks())
    frame = engine.wait_for(lambda: engine.frames_of("registerfunction"), what="registerfunction")[0]
    assert frame["observe"] == {"delivery": "owner", "args_format": CronHooks.args_format}
    engine.deliver(function_registered("cron::on-tick", [observer_info("o", "t", args={"expression": "*/5"})]))
    engine.wait_for(lambda: acks(engine), what="the ack")
    assert index == {"*/5": {"o"}}
    engine.deliver(observer_removed("cron::on-tick", 1, "o"))
    wait_until(lambda: index == {"*/5": set()}, what="the removal")


def test_args_format_on_an_auto_function_is_invalid_observe_descriptor(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    with pytest.raises(InvocationError) as raised:
        client.register_function("auto::fn", lambda d: d, observe=ObserveHooks(args_format={"type": "object"}))
    assert raised.value.code == "INVALID_OBSERVE_DESCRIPTOR"
    with pytest.raises(InvocationError) as raised:
        client.register_function("auto::fn", lambda d: d, observe={"delivery": "push"})
    assert raised.value.code == "INVALID_OBSERVE_DESCRIPTOR"
    assert "auto::fn" not in client._functions


def test_against_an_engine_without_observe_hooks_are_inactive_and_logged_once(
    engine_factory: Any, connect: Any, caplog: pytest.LogCaptureFixture
) -> None:
    legacy = engine_factory("legacy")
    with caplog.at_level(logging.WARNING, logger="iii.iii"):
        client = connect(legacy)
        client.register_function("owner::fn", lambda d: None, observe=ObserveHooks(delivery="owner"))
        client.register_function("plain::fn", lambda d: d)
        frame = legacy.wait_for(
            lambda: [f for f in legacy.frames_of("registerfunction") if f["id"] == "owner::fn"], what="registration"
        )[0]
        assert frame["observe"] == {"delivery": "owner"}  # registered; the engine ignores what it does not know
        legacy.drop()
        wait_until(lambda: len(legacy.connections) == 2 and client._registered_on_connection, what="the reconnect")
    inactive = [r for r in caplog.records if "hooks are inactive" in r.getMessage()]
    assert len(inactive) == 1
    assert "owner::fn" in inactive[0].getMessage()
    assert client._functions["plain::fn"].observe.local_dispatch is False  # no functionregistered: no local dispatch


def test_reports_held_for_a_conflicted_function_are_released_as_inactive(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    client.register_function("held::fn", lambda d: d)
    engine.deliver(function_registered("held::fn", [observer_info("o", "t", capture=["input"])]))
    engine.wait_for(lambda: acks(engine), what="the ack")
    state = client._functions["held::fn"].observe
    # a report waiting for the function to be confirmed on the current connection
    state.confirmed_epoch = -1
    client.trigger({"function_id": "held::fn", "payload": 1})
    wait_until(lambda: client._reporter.size == 1, what="the held report")
    engine.deliver(
        {
            "type": "registrationrejected",
            "code": "FUNCTION_NAMESPACE_CONFLICT",
            "namespace": "default",
            "function_id": "held::fn",
            "owner_worker_id": "other",
        }
    )
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

    ref = client.register_function(
        "conflict::fn",
        lambda d: d,
        observe=ObserveHooks(
            on_observer_added=on_added, on_observer_removed=lambda obs: log.append(f"removed {obs.id}")
        ),
    )
    engine.deliver(function_registered("conflict::fn", [observer_info("fast", "t")]))
    engine.wait_for(lambda: acks(engine), what="the first ack")
    engine.deliver(observer_added("conflict::fn", 1, observer_info("slow", "t")))
    assert entered.wait(5)
    engine.deliver(
        {
            "type": "registrationrejected",
            "code": "FUNCTION_NAMESPACE_CONFLICT",
            "namespace": "default",
            "function_id": "conflict::fn",
            "owner_worker_id": "other",
        }
    )
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

    ref = client.register_function(
        "queued::fn",
        lambda d: d,
        observe=ObserveHooks(
            on_observer_added=on_added, on_observer_removed=lambda obs: log.append(f"removed {obs.id}")
        ),
    )
    engine.deliver(function_registered("queued::fn"))
    engine.deliver(observer_added("queued::fn", 1, observer_info("slow", "t")))
    assert entered.wait(5)
    engine.deliver(observer_added("queued::fn", 2, observer_info("queued", "t")))  # waits behind "slow"
    engine.deliver(
        {
            "type": "registrationrejected",
            "code": "FUNCTION_NAMESPACE_CONFLICT",
            "namespace": "default",
            "function_id": "queued::fn",
            "owner_worker_id": "other",
        }
    )
    wait_until(lambda: not client._functions["queued::fn"].observe.local_dispatch, what="the release")
    release.set()
    settle(client, engine)
    assert log == ["added slow", "slow hook returned", "removed slow"]  # "queued" never ran its hook
    assert acks(engine) == []
    assert ref.is_observed() is False


def test_a_reject_from_another_thread_right_before_activation_wins(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    ref = client.register_function("race::fn", lambda d: d, observe=ObserveHooks(on_observer_added=lambda obs: None))
    state = client._functions["race::fn"].observe
    activate = state._activate

    def reject_first(obs: Observer) -> bool:
        # the owner rejects it from another thread in the window before activation
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

    ref = client.register_function(
        "readd::fn",
        lambda d: None,
        observe=ObserveHooks(
            delivery="owner",
            on_observer_added=lambda obs: log.append(f"added {obs.id}"),
            on_observer_removed=on_removed,
        ),
    )
    engine.deliver(function_registered("readd::fn", [observer_info("o", "t")], revision=1))
    engine.wait_for(lambda: acks(engine), what="the ack")
    ref.observers()[0].reject("GONE", "rejected late")
    engine.wait_for(lambda: len(acks(engine)) == 2, what="the late rejection")
    # a fresh add of the rejected observer: its old self is removed first (the hook blocks) ...
    engine.deliver(observer_added("readd::fn", 2, observer_info("o", "t")))
    assert removing.wait(5)
    # ... and the function is released meanwhile
    engine.deliver(
        {
            "type": "registrationrejected",
            "code": "FUNCTION_NAMESPACE_CONFLICT",
            "namespace": "default",
            "function_id": "readd::fn",
            "owner_worker_id": "other",
        }
    )
    wait_until(lambda: not client._functions["readd::fn"].observe.local_dispatch, what="the release")
    release.set()
    settle(client, engine)
    assert log == ["added o", "removed o"]  # the re-add never ran its hook
    assert len(acks(engine)) == 2  # no success ack after the release
    assert ref.is_observed() is False
