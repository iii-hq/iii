"""``iii.observe`` and its handle, against a fake engine."""

from __future__ import annotations

import asyncio
import threading
import time
from typing import Any

import pytest
from iii_helpers.observability import ReconnectionConfig
from observe_fake_engine import FakeEngine, function_registered, loop_barrier, settle, wait_until

from iii import ObserveRejection
from iii.iii import III


def _state(engine: FakeEngine, observer_id: str, state: str, reason: dict[str, str] | None = None) -> None:
    frame: dict[str, Any] = {"type": "observerstatechanged", "id": observer_id, "state": state, "fires": 0}
    if reason is not None:
        frame["reason"] = reason
    engine.deliver(frame)


def _answer(engine: FakeEngine, observer_id: str, state: str = "pending") -> None:
    """The engine's ``observeresult``: it answers every ``observe`` before any state change."""
    engine.deliver({"type": "observeresult", "id": observer_id, "state": state})


def test_returns_a_pending_handle_synchronously_and_sends_it_after_workerregistered(
    engine_factory: Any, connect: Any
) -> None:
    engine = engine_factory("silent")  # no workerregistered yet
    client = connect(engine, wait_registered=False)
    handle = client.observe({"function_id": "orders::create"}, "orders::audit")
    assert handle.state == "pending"
    assert handle.reason is None
    settle(client, engine)
    assert engine.frames_of("observe") == []
    engine.deliver({"type": "workerregistered", "worker_id": "w-1", "features": ["observe.v1"]})
    [frame] = engine.wait_for(lambda: engine.frames_of("observe"), what="the observe frame")
    assert frame["id"] == handle.id
    assert frame["source"] == {"function_id": "orders::create"}
    assert frame["target"] == {"function_id": "orders::audit"}


def test_sends_the_functions_before_the_observers_on_every_connection(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    client.register_function("local::target", lambda d: None)
    handle = client.observe({"function_id": "orders::create"}, "local::target")
    engine.wait_for(lambda: engine.frames_of("observe"), what="the first observe")
    engine.drop()
    wait_until(lambda: len(engine.connections) == 2, what="the reconnect")
    engine.wait_for(lambda: engine.frames_of("observe", 1), what="the replay")
    types = [f["type"] for f in engine.frames(1) if f["type"] in ("registerfunction", "observe")]
    assert types == ["registerfunction", "observe"]
    assert engine.frames_of("observe", 1)[0]["id"] == handle.id  # the same id


def test_fills_the_namespace_from_the_worker_and_default_for_engine_builtins(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine, namespace="billing")
    client.observe({"function_id": "invoices::close"}, "billing::audit")
    client.observe({"function_id": "engine::on-functions-changed"}, {"function_id": "x::y", "namespace": "other"})
    a, b = engine.wait_for(lambda: len(engine.frames_of("observe")) == 2 and engine.frames_of("observe"), what="both")
    assert a["source"]["namespace"] == "billing"
    assert a["target"]["namespace"] == "billing"
    assert b["source"]["namespace"] == "default"
    assert b["target"] == {"function_id": "x::y", "namespace": "other"}
    with pytest.raises(ValueError):
        client.observe({"function_id": "a::b", "namespace": " "}, "c::d")


def test_follows_observeresult_and_observerstatechanged_and_ready_resolves_on_active(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    handle = client.observe({"function_id": "f::x"}, "t::x")
    changes: list[tuple[str, Any]] = []
    off = handle.on_state_change(lambda state, reason: changes.append((state, reason)))
    engine.wait_for(lambda: engine.frames_of("observe"), what="the observe frame")
    engine.deliver({"type": "observeresult", "id": handle.id, "state": "pending", "warnings": ["output.x: open"]})
    waiter = threading.Thread(target=handle.ready, kwargs={"timeout": 5})
    waiter.start()
    _state(engine, handle.id, "active")
    waiter.join(5)
    assert not waiter.is_alive()
    assert handle.state == "active"
    handle.ready()  # settled: returns at once

    async def ready_from_a_loop() -> None:
        await handle.ready_async()

    asyncio.run(ready_from_a_loop())
    _state(engine, handle.id, "suspended", {"code": "OBSERVER_DISCONNECTED", "message": "gone"})
    wait_until(lambda: handle.state == "suspended", what="suspended")
    assert handle.reason == {"code": "OBSERVER_DISCONNECTED", "message": "gone"}
    wait_until(lambda: len(changes) == 2, what="the listener")  # delivered asynchronously, in order
    off()
    _state(engine, handle.id, "active")
    wait_until(lambda: handle.state == "active", what="active again")
    assert [c[0] for c in changes] == ["active", "suspended"]


def test_ready_raises_observe_rejection_with_the_code_when_the_engine_answers_error(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    handle = client.observe({"function_id": "f::x", "args": {"bad": True}}, "t::x")
    engine.wait_for(lambda: engine.frames_of("observe"), what="the observe frame")

    async def wait_async() -> ObserveRejection:
        with pytest.raises(ObserveRejection) as raised:
            await handle.ready_async()
        return raised.value

    result: list[ObserveRejection] = []
    waiter = threading.Thread(target=lambda: result.append(asyncio.run(wait_async())))
    waiter.start()  # whether it starts waiting before or after the answer, it raises the same
    engine.deliver(
        {
            "type": "observeresult",
            "id": handle.id,
            "state": "error",
            "error": {"code": "INVALID_ARGS", "message": "f::x takes no args"},
        }
    )
    waiter.join(5)
    assert result and result[0].code == "INVALID_ARGS"
    with pytest.raises(ObserveRejection) as raised:
        handle.ready()
    assert (raised.value.code, raised.value.message) == ("INVALID_ARGS", "f::x takes no args")
    assert handle.state == "error"


def test_goes_to_error_with_unsupported_against_an_engine_without_observe(engine_factory: Any, connect: Any) -> None:
    legacy = engine_factory("legacy")
    client = connect(legacy)
    before = client.observe({"function_id": "f::x"}, "t::x")
    with pytest.raises(ObserveRejection) as raised:
        before.ready(timeout=5)
    assert raised.value.code == "UNSUPPORTED"
    settle(client, legacy)
    assert legacy.frames_of("observe") == []


def test_unsupported_for_observers_made_before_the_connection_to_an_old_engine(
    engine_factory: Any, connect: Any
) -> None:
    legacy = engine_factory("silent")
    client = connect(legacy, wait_registered=False)
    handle = client.observe({"function_id": "f::x"}, "t::x")
    legacy.deliver({"type": "workerregistered", "worker_id": "w"})  # no features: a pre-0.25 engine
    with pytest.raises(ObserveRejection) as raised:
        handle.ready(timeout=5)
    assert raised.value.code == "UNSUPPORTED"


def test_replays_with_the_same_id_after_a_reconnect_and_a_disposed_observer_is_not_replayed(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    kept = client.observe({"function_id": "f::a"}, "t::a")
    gone = client.observe({"function_id": "f::b"}, "t::b")
    engine.wait_for(lambda: len(engine.frames_of("observe")) == 2, what="both observers")
    gone.dispose()
    engine.wait_for(lambda: engine.frames_of("unobserve"), what="the unobserve")
    assert engine.frames_of("unobserve")[0] == {"type": "unobserve", "id": gone.id}
    assert gone.state == "disposed"
    assert gone.reason == {"code": "UNOBSERVED", "message": "disposed by the observing worker"}
    engine.drop()
    wait_until(lambda: len(engine.connections) == 2, what="the reconnect")
    engine.wait_for(lambda: engine.frames_of("observe", 1), what="the replay")
    settle(client, engine)
    assert [f["id"] for f in engine.frames_of("observe", 1)] == [kept.id]
    with pytest.raises(ObserveRejection) as raised:
        gone.ready()
    assert raised.value.code == "UNOBSERVED"


def test_queues_the_unobserve_of_an_observer_disposed_while_disconnected(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    handle = client.observe({"function_id": "f::a"}, "t::a")
    engine.wait_for(lambda: engine.frames_of("observe"), what="the observe")
    engine.stop()
    wait_until(lambda: client.get_connection_state() != "connected", what="the disconnect")
    handle.dispose()
    loop_barrier(client)
    queued = [f for f in client._queue if f.get("type") == "unobserve"]
    assert queued == [{"type": "unobserve", "id": handle.id}]


def test_queued_observe_frames_are_held_until_the_engine_says_it_supports_them(
    engine_factory: Any, connect: Any
) -> None:
    engine = engine_factory()
    # a slow reconnect, so the dispose below happens while disconnected
    client = connect(
        engine, reconnection_config=ReconnectionConfig(initial_delay_ms=400, max_delay_ms=400, jitter_factor=0.0)
    )
    handle = client.observe({"function_id": "f::a"}, "t::a")
    engine.wait_for(lambda: engine.frames_of("observe"), what="the observe")
    # disconnected: the unobserve waits in the send queue
    client._run_on_loop(_close(client))
    wait_until(lambda: client.get_connection_state() != "connected", what="the disconnect")
    engine.features = "legacy"  # the engine came back without observe
    handle.dispose()
    wait_until(lambda: len(engine.connections) == 2 and client._registered_on_connection, what="the reconnect")
    settle(client, engine)
    assert engine.frames_of("unobserve") == []  # dropped: that engine would not decode it
    assert client._held_observe_frames == []


async def _close(client: III) -> None:
    assert client._ws is not None
    client._ws.transport.abort()


def test_forgets_an_observer_the_engine_disposed(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    handle = client.observe({"function_id": "f::a"}, "t::a", {"max_fires": 1})
    [frame] = engine.wait_for(lambda: engine.frames_of("observe"), what="the observe")
    assert frame["max_fires"] == 1
    _answer(engine, handle.id, "active")
    _state(engine, handle.id, "disposed", {"code": "MAX_FIRES", "message": "fired 1 time"})
    wait_until(lambda: handle.state == "disposed", what="disposed")
    assert handle.reason == {"code": "MAX_FIRES", "message": "fired 1 time"}
    assert handle.id not in client._observers
    engine.drop()
    wait_until(lambda: len(engine.connections) == 2 and client._registered_on_connection, what="the reconnect")
    settle(client, engine)
    assert engine.frames_of("observe", 1) == []


def test_the_source_is_a_function_id_or_a_dict(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    client.observe("orders::create", "alerts::big-order", {"id": "short"})
    client.observe({"function_id": "orders::create"}, "alerts::big-order", {"id": "long"})
    short, long = engine.wait_for(
        lambda: len(engine.frames_of("observe")) == 2 and engine.frames_of("observe"), what="both frames"
    )
    assert {**short, "id": "long"} == long
    with pytest.raises(TypeError):
        client.observe(["orders::create"], "alerts::big-order")  # type: ignore[arg-type]


def test_refuses_a_held_id_and_a_missing_source_or_target(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    holder = client.observe({"function_id": "f::a"}, "t::a", {"id": "fixed"})
    engine.wait_for(lambda: engine.frames_of("observe"), what="the observe")
    for state in ("pending", "error"):  # held in any state, until disposed
        if state == "error":
            engine.deliver(
                {
                    "type": "observeresult",
                    "id": "fixed",
                    "state": "error",
                    "error": {"code": "INVALID_ARGS", "message": "x"},
                }
            )
            wait_until(lambda: holder.state == "error", what="error")
        with pytest.raises(ObserveRejection) as raised:
            client.observe({"function_id": "f::b"}, "t::b", {"id": "fixed"})
        assert (raised.value.code, raised.value.message) == (
            "OBSERVER_ID_CONFLICT",
            "observe: observer id already in use: fixed",
        )
        assert client._observers["fixed"] is holder
    settle(client, engine)
    assert len(engine.frames_of("observe")) == 1  # the refused observe sent nothing
    with pytest.raises(ValueError):
        client.observe({"function_id": ""}, "t::a")
    with pytest.raises(ValueError):
        client.observe({"function_id": "f::a"}, "")
    with pytest.raises(ValueError):
        client.observe({"function_id": "f::a"}, {"namespace": "x"})  # type: ignore[typeddict-item]
    with pytest.raises(TypeError):
        client.observe({"function_id": "f::a", "filters": {"input.when": {"$eq": object()}}}, "t::a")


def test_concurrent_observe_with_the_same_id_lets_exactly_one_win(
    engine: FakeEngine, connect: Any, monkeypatch: pytest.MonkeyPatch
) -> None:
    client = connect(engine)
    import iii.iii as iii_module

    real = iii_module.build_observe_message
    gate = threading.Barrier(2, timeout=0.5)

    def slow_build(*args: Any, **kwargs: Any) -> Any:
        try:
            gate.wait()  # both threads are past the duplicate check
        except threading.BrokenBarrierError:
            pass  # one thread did not reach the barrier in time
        return real(*args, **kwargs)

    monkeypatch.setattr(iii_module, "build_observe_message", slow_build)
    results: list[object] = []

    def call() -> None:
        try:
            results.append(client.observe({"function_id": "f::a"}, "t::a", {"id": "dup"}))
        except ObserveRejection as exc:
            results.append(exc)

    threads = [threading.Thread(target=call) for _ in range(2)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert sum(isinstance(r, ObserveRejection) for r in results) == 1
    assert len(client._observers) == 1
    settle(client, engine)
    assert len(engine.frames_of("observe")) == 1


def test_a_state_listener_that_raises_does_not_stop_the_others(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    handle = client.observe({"function_id": "f::a"}, "t::a")
    seen: list[str] = []

    def bad(state: str, reason: Any) -> None:
        raise RuntimeError("listener bug")

    handle.on_state_change(bad)
    handle.on_state_change(lambda state, reason: seen.append(state))
    engine.wait_for(lambda: engine.frames_of("observe"), what="the observe")
    _answer(engine, handle.id)
    _state(engine, handle.id, "active")
    wait_until(lambda: seen == ["active"], what="the second listener")


def test_ready_from_the_sdk_loop_thread_is_refused(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    handle = client.observe({"function_id": "f::a"}, "t::a")

    async def on_loop() -> None:
        handle.ready(timeout=0.1)

    with pytest.raises(RuntimeError):
        client._run_on_loop(on_loop())
    with pytest.raises(TimeoutError):
        handle.ready(timeout=0.05)


def test_state_listeners_run_off_the_event_loop_in_order_and_may_call_the_sync_api(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    client.register_function("local::fn", lambda d: "called")
    handle = client.observe({"function_id": "f::a"}, "t::a")
    seen: list[tuple[str, str, Any]] = []

    def listener(state: str, reason: Any) -> None:
        seen.append((state, threading.current_thread().name, reason))

    handle.on_state_change(listener)
    engine.wait_for(lambda: engine.frames_of("observe"), what="the observe")
    _answer(engine, handle.id)
    for state in ("active", "suspended", "active"):
        _state(engine, handle.id, state)
    wait_until(lambda: len(seen) == 3, what="three deliveries")
    assert [s[0] for s in seen] == ["active", "suspended", "active"]
    assert all(name != client._thread.name for _, name, _ in seen)

    # the sync API raises on the SDK loop thread: a listener runs elsewhere, so it may call it
    engine.deliver(function_registered("local::fn"))
    wait_until(lambda: client._functions["local::fn"].observe.local_dispatch, what="local dispatch")
    results: list[Any] = []
    handle.on_state_change(
        lambda state, reason: results.append(client.call({"function_id": "local::fn", "payload": {}}))
    )
    _state(engine, handle.id, "suspended")
    wait_until(lambda: results == ["called"], what="a sync call from a listener")


# observer ids (decision 56): the engine's answers are attributed by frame order


def _types_and_ids(engine: FakeEngine, connection: int | None = None) -> list[tuple[str, Any]]:
    return [(f["type"], f.get("id")) for f in engine.frames(connection) if f["type"] in ("observe", "unobserve")]


@pytest.mark.parametrize("answered_first", [True, False], ids=["answered", "unanswered"])
def test_an_id_is_free_at_once_after_dispose_and_the_late_disposed_reaches_the_earlier_handle(
    engine: FakeEngine, connect: Any, answered_first: bool
) -> None:
    client = connect(engine)
    first = client.observe({"function_id": "f::a"}, "t::a", {"id": "queue-consumer"})
    engine.wait_for(lambda: engine.frames_of("observe"), what="the first observe")
    if answered_first:
        _answer(engine, "queue-consumer", "active")
        wait_until(lambda: first.state == "active", what="active")
    first.dispose()
    second = client.observe({"function_id": "f::a"}, "t::a", {"id": "queue-consumer"})
    assert client._observers["queue-consumer"] is second
    engine.wait_for(lambda: len(engine.frames_of("observe")) == 2, what="the second observe")
    assert _types_and_ids(engine) == [
        ("observe", "queue-consumer"),
        ("unobserve", "queue-consumer"),
        ("observe", "queue-consumer"),
    ]
    # what the engine sends, in the order it read the frames
    if not answered_first:
        _answer(engine, "queue-consumer", "active")
    _state(engine, "queue-consumer", "disposed", {"code": "UNOBSERVED", "message": "unobserved"})
    _answer(engine, "queue-consumer", "active")
    wait_until(lambda: second.state == "active", what="the second observer active")
    settle(client, engine)
    assert second.state == "active"
    assert client._observers["queue-consumer"] is second
    assert client._observe_sent["queue-consumer"] == [second]
    assert first.reason == {"code": "UNOBSERVED", "message": "disposed by the observing worker"}
    second.dispose()
    engine.wait_for(lambda: len(engine.frames_of("unobserve")) == 2, what="the second unobserve")


def test_a_state_change_before_any_answer_is_dropped(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    handle = client.observe({"function_id": "f::a"}, "t::a", {"id": "x"})
    engine.wait_for(lambda: engine.frames_of("observe"), what="the observe")
    _answer(engine, "x", "active")
    wait_until(lambda: handle.state == "active", what="active")
    engine.drop()
    wait_until(lambda: len(engine.connections) == 2, what="the reconnect")
    engine.wait_for(lambda: engine.frames_of("observe", 1), what="the replay")
    # a reattached connection hears its observer resume before the replay's answer
    _state(engine, "x", "disposed", {"code": "MAX_FIRES", "message": "fired 1 time"})
    settle(client, engine)
    assert handle.state == "active"
    assert client._observers["x"] is handle
    _answer(engine, "x", "suspended")  # the replay's answer carries the current state
    wait_until(lambda: handle.state == "suspended", what="the replay's answer")
    _state(engine, "x", "active")
    wait_until(lambda: handle.state == "active", what="the state change after the answer")


def test_an_answer_with_no_observe_waiting_for_it_is_dropped(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    handle = client.observe({"function_id": "f::a"}, "t::a", {"id": "x"})
    engine.wait_for(lambda: engine.frames_of("observe"), what="the observe")
    _answer(engine, "x", "active")
    _answer(engine, "x", "error")  # a second answer: no observe of the id waits for one
    _answer(engine, "other", "active")  # an id this client never sent
    settle(client, engine)
    assert handle.state == "active"


def test_queued_unobserves_go_out_before_the_replays(engine_factory: Any, connect: Any) -> None:
    engine = engine_factory()
    client = connect(
        engine, reconnection_config=ReconnectionConfig(initial_delay_ms=400, max_delay_ms=400, jitter_factor=0.0)
    )
    first = client.observe({"function_id": "f::a"}, "t::a", {"id": "x"})
    kept = client.observe({"function_id": "f::b"}, "t::b", {"id": "y"})
    engine.wait_for(lambda: len(engine.frames_of("observe")) == 2, what="both observes")
    client._run_on_loop(_close(client))
    wait_until(lambda: client.get_connection_state() != "connected", what="the disconnect")
    first.dispose()
    second = client.observe({"function_id": "f::a"}, "t::a", {"id": "x"})
    wait_until(lambda: len(engine.connections) == 2 and client._registered_on_connection, what="the reconnect")
    engine.wait_for(lambda: len(engine.frames_of("observe", 1)) == 2, what="the replays")
    settle(client, engine)
    sent = _types_and_ids(engine, 1)
    assert sent[0] == ("unobserve", "x")
    assert sorted(sent[1:]) == [("observe", "x"), ("observe", "y")]
    assert client._observers == {"y": kept, "x": second}


def test_a_handle_is_sent_once_per_connection(engine_factory: Any, connect: Any) -> None:
    engine = engine_factory("silent")
    client = connect(engine, wait_registered=False)
    handles = [client.observe({"function_id": "f::a"}, "t::a") for _ in range(20)]
    engine.deliver({"type": "workerregistered", "worker_id": "w-1", "features": ["observe.v1"]})
    handles += [client.observe({"function_id": "f::a"}, "t::a") for _ in range(20)]
    engine.wait_for(lambda: len(engine.frames_of("observe")) >= 40, what="every observe")
    settle(client, engine)
    assert sorted(f["id"] for f in engine.frames_of("observe")) == sorted(h.id for h in handles)


#: Rounds of the dispose race: enough to hit the window most runs at a 1 µs
#: switch interval (about 2 s).
RACE_ROUNDS = 2000


def test_dispose_racing_the_engines_disposed_never_raises(engine: FakeEngine, connect: Any) -> None:
    import json
    import sys

    client = connect(engine)
    interval = sys.getswitchinterval()
    sys.setswitchinterval(1e-6)
    errors: list[BaseException] = []
    try:
        for round_ in range(RACE_ROUNDS):
            observer_id = f"race-{round_}"
            handle = client.observe({"function_id": "f::a"}, "t::a", {"id": observer_id})
            loop_barrier(client)  # sent
            gate = threading.Barrier(2, timeout=5)
            answer = json.dumps({"type": "observeresult", "id": observer_id, "state": "active"})
            disposed = json.dumps(
                {
                    "type": "observerstatechanged",
                    "id": observer_id,
                    "state": "disposed",
                    "fires": 1,
                    "reason": {"code": "MAX_FIRES", "message": "fired 1 time"},
                }
            )

            async def engine_side(g: threading.Barrier = gate, a: str = answer, d: str = disposed) -> None:
                await client._handle_message(a)
                g.wait()
                await client._handle_message(d)

            def user_side(h: Any = handle, g: threading.Barrier = gate) -> None:
                g.wait()
                try:
                    h.dispose()
                except BaseException as error:  # noqa: BLE001
                    errors.append(error)

            done = asyncio.run_coroutine_threadsafe(engine_side(), client._loop)
            user = threading.Thread(target=user_side)
            user.start()
            user.join(5)
            done.result(5)
            assert handle.state == "disposed"
            assert observer_id not in client._observers
    finally:
        sys.setswitchinterval(interval)
    assert errors == []


# a refused observe: the engine's refusal is the handle's final answer


#: Well under any wait for the engine: a dispose() that sends nothing, or
#: whose unobserve is already on the wire, returns at once.
DISPOSE_RETURNS_WITHIN_S = 0.5


def _refuse(engine: FakeEngine, observer_id: str, code: str = "FORBIDDEN") -> None:
    """The engine's ``observeresult`` for an ``observe`` it refused (it stores nothing)."""
    engine.deliver(
        {"type": "observeresult", "id": observer_id, "state": "error", "error": {"code": code, "message": "refused"}}
    )


def _timed_dispose(handle: Any) -> float:
    started = time.monotonic()
    handle.dispose()
    return time.monotonic() - started


@pytest.mark.parametrize("code", ["FORBIDDEN", "INVALID_ARGS", "OBSERVER_ID_CONFLICT", "LIMIT_EXCEEDED"])
def test_a_refusal_is_final_and_a_dispose_after_it_sends_nothing(engine: FakeEngine, connect: Any, code: str) -> None:
    client = connect(engine)
    handle = client.observe({"function_id": "f::a"}, "t::a", {"id": "x"})
    engine.wait_for(lambda: engine.frames_of("observe"), what="the observe")
    _refuse(engine, "x", code)
    wait_until(lambda: handle.state == "error", what="the refusal")
    with pytest.raises(ObserveRejection) as raised:
        handle.ready(timeout=1)
    assert raised.value.code == code
    assert "x" not in client._observe_sent  # it left the id's list
    assert client._observers["x"] is handle  # and holds the id until its dispose()
    with pytest.raises(ObserveRejection) as conflict:
        client.observe({"function_id": "f::a"}, "t::a", {"id": "x"})
    assert conflict.value.code == "OBSERVER_ID_CONFLICT"
    _state(engine, "x", "active")  # nothing the engine says about the id reaches it
    settle(client, engine)
    assert handle.state == "error"
    assert _timed_dispose(handle) < DISPOSE_RETURNS_WITHIN_S
    settle(client, engine)
    assert engine.frames_of("unobserve") == []
    assert handle.state == "disposed"
    assert "x" not in client._observers


def test_a_dispose_sent_before_the_refusal_returns_at_once_and_the_refusal_answers_it(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    handle = client.observe({"function_id": "f::a"}, "t::a", {"id": "x"})
    engine.wait_for(lambda: engine.frames_of("observe"), what="the observe")
    assert _timed_dispose(handle) < DISPOSE_RETURNS_WITHIN_S
    engine.wait_for(lambda: engine.frames_of("unobserve"), what="the unobserve")
    assert client._observe_sent["x"] == [handle]  # listed until its answer
    _refuse(engine, "x")  # the engine ignores the unobserve: it holds nothing for x
    wait_until(lambda: "x" not in client._observe_sent, what="the refusal")
    settle(client, engine)
    assert handle.state == "disposed"
    assert handle.reason == {"code": "UNOBSERVED", "message": "disposed by the observing worker"}
    assert _types_and_ids(engine) == [("observe", "x"), ("unobserve", "x")]
    handle.dispose()  # again: nothing
    settle(client, engine)
    assert len(engine.frames_of("unobserve")) == 1


def test_after_a_refusal_and_dispose_a_new_observe_of_the_id_works(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    refused = client.observe({"function_id": "f::a"}, "t::a", {"id": "x"})
    kept = client.observe({"function_id": "f::b"}, "t::b", {"id": "y"})
    engine.wait_for(lambda: len(engine.frames_of("observe")) == 2, what="both observes")
    _refuse(engine, "x")
    wait_until(lambda: refused.state == "error", what="the refusal")
    refused.dispose()
    second = client.observe({"function_id": "f::a", "args": {"fixed": True}}, "t::a", {"id": "x"})
    engine.wait_for(lambda: len(engine.frames_of("observe")) == 3, what="the second observe")
    assert _types_and_ids(engine) == [("observe", "x"), ("observe", "y"), ("observe", "x")]
    _answer(engine, "x", "pending")
    _state(engine, "x", "active")
    wait_until(lambda: second.state == "active", what="the second observer active")
    assert refused.state == "disposed"
    assert client._observe_sent["x"] == [second]
    engine.drop()  # the replay: the observers that hold their ids
    wait_until(lambda: len(engine.connections) == 2 and client._registered_on_connection, what="the reconnect")
    engine.wait_for(lambda: len(engine.frames_of("observe", 1)) == 2, what="the replays")
    settle(client, engine)
    replays = {f["id"]: f for f in engine.frames_of("observe", 1)}
    assert sorted(replays) == ["x", "y"]
    assert replays["x"]["source"]["args"] == {"fixed": True}  # the second's
    assert client._observers == {"y": kept, "x": second}
    second.dispose()
    engine.wait_for(lambda: engine.frames_of("unobserve", 1), what="the second observer's unobserve")


def test_a_refusal_answers_the_older_handle_while_a_newer_one_of_the_id_is_pending(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    first = client.observe({"function_id": "f::a"}, "t::a", {"id": "x"})
    engine.wait_for(lambda: engine.frames_of("observe"), what="the first observe")
    assert _timed_dispose(first) < DISPOSE_RETURNS_WITHIN_S
    second = client.observe({"function_id": "f::a"}, "t::a", {"id": "x"})
    engine.wait_for(lambda: len(engine.frames_of("observe")) == 2, what="the second observe")
    assert _types_and_ids(engine) == [("observe", "x"), ("unobserve", "x"), ("observe", "x")]
    # the engine refuses the first, ignores the unobserve and accepts the second
    _refuse(engine, "x")
    wait_until(lambda: client._observe_sent.get("x") == [second], what="the refusal")
    _state(engine, "x", "active")  # before the second's answer: dropped
    settle(client, engine)
    assert second.state == "pending"
    assert first.reason == {"code": "UNOBSERVED", "message": "disposed by the observing worker"}
    _answer(engine, "x", "active")
    wait_until(lambda: second.state == "active", what="the second observer's answer")
    second.ready(timeout=1)
    _state(engine, "x", "suspended")
    wait_until(lambda: second.state == "suspended", what="a state change after the answer")
    assert client._observe_sent["x"] == [second]


def test_a_refusal_of_the_newer_handle_after_the_older_ones_disposed(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    first = client.observe({"function_id": "f::a"}, "t::a", {"id": "x"})
    engine.wait_for(lambda: engine.frames_of("observe"), what="the first observe")
    _answer(engine, "x", "active")
    wait_until(lambda: first.state == "active", what="active")
    first.dispose()
    second = client.observe({"function_id": "f::a", "args": {"bad": True}}, "t::a", {"id": "x"})
    engine.wait_for(lambda: len(engine.frames_of("observe")) == 2, what="the second observe")
    _state(engine, "x", "disposed", {"code": "UNOBSERVED", "message": "unobserved"})  # the first's
    _refuse(engine, "x", "INVALID_ARGS")  # the second's
    wait_until(lambda: second.state == "error", what="the refusal")
    assert second.reason == {"code": "INVALID_ARGS", "message": "refused"}
    assert "x" not in client._observe_sent
    assert _timed_dispose(second) < DISPOSE_RETURNS_WITHIN_S
    settle(client, engine)
    assert len(engine.frames_of("unobserve")) == 1  # the first's only


def test_a_refused_observe_is_not_replayed(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    refused = client.observe({"function_id": "f::a"}, "t::a", {"id": "x"})
    kept = client.observe({"function_id": "f::b"}, "t::b", {"id": "y"})
    engine.wait_for(lambda: len(engine.frames_of("observe")) == 2, what="both observes")
    _refuse(engine, "x")
    wait_until(lambda: refused.state == "error", what="the refusal")
    engine.drop()
    wait_until(lambda: len(engine.connections) == 2 and client._registered_on_connection, what="the reconnect")
    engine.wait_for(lambda: engine.frames_of("observe", 1), what="the replay")
    settle(client, engine)
    assert [f["id"] for f in engine.frames_of("observe", 1)] == ["y"]
    assert refused.reason == {"code": "FORBIDDEN", "message": "refused"}
    assert client._observers == {"x": refused, "y": kept}
    refused.dispose()
    settle(client, engine)
    assert engine.frames_of("unobserve") == []


def test_an_error_answer_to_the_replay_of_an_accepted_observer_keeps_it_listed(
    engine: FakeEngine, connect: Any
) -> None:
    """The engine answers the replay of an observer it holds in ``error`` with
    that state: only an ``unobserve`` removes it there."""
    client = connect(engine)
    handle = client.observe({"function_id": "f::a"}, "t::a", {"id": "x"})
    engine.wait_for(lambda: engine.frames_of("observe"), what="the observe")
    _answer(engine, "x", "pending")
    _state(engine, "x", "error", {"code": "OBSERVE_REJECTED", "message": "the owner rejected it"})
    wait_until(lambda: handle.state == "error", what="the owner's rejection")
    engine.drop()
    wait_until(lambda: len(engine.connections) == 2, what="the reconnect")
    engine.wait_for(lambda: engine.frames_of("observe", 1), what="the replay")
    engine.deliver(
        {
            "type": "observeresult",
            "id": "x",
            "state": "error",
            "error": {"code": "OBSERVE_REJECTED", "message": "the owner rejected it"},
        }
    )
    settle(client, engine)
    assert client._observe_sent["x"] == [handle]
    assert _timed_dispose(handle) < DISPOSE_RETURNS_WITHIN_S
    engine.wait_for(lambda: engine.frames_of("unobserve", 1), what="the unobserve")
    _state(engine, "x", "disposed", {"code": "UNOBSERVED", "message": "unobserved"})
    wait_until(lambda: "x" not in client._observe_sent, what="the engine's disposed")


def test_an_error_answer_to_a_replay_whose_first_answer_was_lost_keeps_it_listed(
    engine: FakeEngine, connect: Any
) -> None:
    """The engine created the observer and its owner rejected it, but the
    connection dropped before either frame reached the SDK. The reattached
    connection's replay is answered with the held observer's ``error``: the
    handle stays listed, is replayed, and its dispose() unobserves it."""
    client = connect(engine)
    handle = client.observe({"function_id": "f::a"}, "t::a", {"id": "x"})
    engine.wait_for(lambda: engine.frames_of("observe"), what="the observe")
    engine.drop()  # no answer for x reached the SDK
    wait_until(lambda: len(engine.connections) == 2 and client._registered_on_connection, what="the reconnect")
    engine.wait_for(lambda: engine.frames_of("observe", 1), what="the replay")
    rejected = {"code": "OBSERVE_REJECTED", "message": "the owner rejected it"}
    engine.deliver({"type": "observeresult", "id": "x", "state": "error", "error": rejected})
    wait_until(lambda: handle.state == "error", what="the replay's answer")
    assert handle.reason == rejected
    assert client._observe_sent["x"] == [handle]
    engine.drop()
    wait_until(lambda: len(engine.connections) == 3 and client._registered_on_connection, what="the second reconnect")
    engine.wait_for(lambda: engine.frames_of("observe", 2), what="the second replay")
    assert _timed_dispose(handle) < DISPOSE_RETURNS_WITHIN_S
    engine.wait_for(lambda: engine.frames_of("unobserve", 2), what="the unobserve")
    assert _types_and_ids(engine) == [("observe", "x"), ("observe", "x"), ("observe", "x"), ("unobserve", "x")]


def test_a_refused_handle_keeps_its_refusal_against_an_engine_without_observe(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    refused = client.observe({"function_id": "f::a"}, "t::a", {"id": "x"})
    kept = client.observe({"function_id": "f::b"}, "t::b", {"id": "y"})
    engine.wait_for(lambda: len(engine.frames_of("observe")) == 2, what="both observes")
    _refuse(engine, "x", "INVALID_ARGS")
    _answer(engine, "y", "active")
    wait_until(lambda: refused.state == "error" and kept.state == "active", what="both answers")
    engine.features = "legacy"  # the engine comes back without observe
    engine.drop()
    wait_until(lambda: len(engine.connections) == 2, what="the reconnect")
    wait_until(lambda: kept.state == "error", what="the held observer's UNSUPPORTED")
    settle(client, engine)
    assert kept.reason is not None and kept.reason["code"] == "UNSUPPORTED"
    assert refused.reason == {"code": "INVALID_ARGS", "message": "refused"}
    assert engine.frames_of("observe", 1) == []
    refused.dispose()
    settle(client, engine)
    assert engine.frames_of("unobserve") == []


#: Rounds of the refusal race (about 0.5 s at a 1 µs switch interval).
REFUSAL_RACE_ROUNDS = 500


def test_dispose_racing_the_refusal_leaves_nothing_listed_or_held(engine: FakeEngine, connect: Any) -> None:
    import json
    import sys

    client = connect(engine)
    interval = sys.getswitchinterval()
    sys.setswitchinterval(1e-6)
    errors: list[BaseException] = []
    try:
        for round_ in range(REFUSAL_RACE_ROUNDS):
            observer_id = f"refused-{round_}"
            handle = client.observe({"function_id": "f::a"}, "t::a", {"id": observer_id})
            loop_barrier(client)  # sent
            gate = threading.Barrier(2, timeout=5)
            refusal = json.dumps(
                {
                    "type": "observeresult",
                    "id": observer_id,
                    "state": "error",
                    "error": {"code": "FORBIDDEN", "message": "refused"},
                }
            )

            async def engine_side(g: threading.Barrier = gate, r: str = refusal) -> None:
                g.wait()
                await client._handle_message(r)

            def user_side(h: Any = handle, g: threading.Barrier = gate) -> None:
                g.wait()
                try:
                    h.dispose()
                except BaseException as error:  # noqa: BLE001
                    errors.append(error)

            done = asyncio.run_coroutine_threadsafe(engine_side(), client._loop)
            user = threading.Thread(target=user_side)
            user.start()
            user.join(5)
            done.result(5)
            loop_barrier(client)
            assert handle.state == "disposed"
            assert observer_id not in client._observers
            assert observer_id not in client._observe_sent
    finally:
        sys.setswitchinterval(interval)
    assert errors == []
    settle(client, engine)
    assert len(engine.frames_of("unobserve")) <= REFUSAL_RACE_ROUNDS
