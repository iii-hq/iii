"""``iii.observe`` and its handle (sdk.md §2, §4.5), against a fake engine."""

from __future__ import annotations

import asyncio
import threading
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
    _state(engine, handle.id, "disposed", {"code": "MAX_FIRES", "message": "fired 1 time"})
    wait_until(lambda: handle.state == "disposed", what="disposed")
    assert handle.reason == {"code": "MAX_FIRES", "message": "fired 1 time"}
    assert handle.id not in client._observers
    engine.drop()
    wait_until(lambda: len(engine.connections) == 2 and client._registered_on_connection, what="the reconnect")
    settle(client, engine)
    assert engine.frames_of("observe", 1) == []


def test_refuses_a_reused_id_and_a_missing_source_or_target(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    client.observe({"function_id": "f::a"}, "t::a", {"id": "fixed"})
    with pytest.raises(ValueError):
        client.observe({"function_id": "f::a"}, "t::a", {"id": "fixed"})
    with pytest.raises(ValueError):
        client.observe({"function_id": ""}, "t::a")
    with pytest.raises(ValueError):
        client.observe({"function_id": "f::a"}, "")
    with pytest.raises(ValueError):
        client.observe({"function_id": "f::a"}, {"namespace": "x"})  # type: ignore[typeddict-item]
    with pytest.raises(TypeError):
        client.observe({"function_id": "f::a", "filters": {"input.when": {"$eq": object()}}}, "t::a")


def test_a_state_listener_that_raises_does_not_stop_the_others(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    handle = client.observe({"function_id": "f::a"}, "t::a")
    seen: list[str] = []

    def bad(state: str, reason: Any) -> None:
        raise RuntimeError("listener bug")

    handle.on_state_change(bad)
    handle.on_state_change(lambda state, reason: seen.append(state))
    engine.wait_for(lambda: engine.frames_of("observe"), what="the observe")
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
        lambda state, reason: results.append(client.trigger({"function_id": "local::fn", "payload": {}}))
    )
    _state(engine, handle.id, "suspended")
    wait_until(lambda: results == ["called"], what="a sync call from a listener")
