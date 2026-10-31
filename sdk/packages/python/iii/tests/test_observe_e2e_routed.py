"""``iii.observe`` against the real engine, on the routed path: every call
goes through the engine, which evaluates the observers' ``on`` and filters
from the input it forwarded and the host's answer (decision 55). The host
runs the function's match and deliver hooks only."""

from __future__ import annotations

import time
from typing import Any

import pytest
from observe_e2e_utils import (
    FrameCounter,
    collector,
    drop_connection,
    exactly,
    local_dispatch_ready,
    settle_engine,
    wait_for,
)

from iii import ObserveRejection

ALL = ["input", "output", "error", "caller", "metadata"]


def test_activates_the_target_with_the_envelope_when_filters_on_input_output_and_caller_match(
    worker: Any, fid: Any
) -> None:
    owner = worker("routed-owner")
    watcher = worker("routed-watcher")
    owner.register_function(fid("py-e2e::charge"), lambda data: {"charged": data["amount"]})
    got = collector(watcher, fid("py-e2e::on-big-charge"))
    watcher.register_function(
        fid("py-e2e::checkout"),
        lambda amount: watcher.call({"function_id": fid("py-e2e::charge"), "payload": {"amount": amount}}),
    )
    handle = watcher.observe(
        {
            "function_id": fid("py-e2e::charge"),
            "filters": {
                "input.amount": {"$gte": 500},
                "output.charged": {"$exists": True},
                "caller.function_id": {"$eq": fid("py-e2e::checkout")},
            },
            "capture": ALL,
        },
        fid("py-e2e::on-big-charge"),
        {"metadata": {"team": "payments"}},
    )
    handle.ready(timeout=10)
    assert handle.state == "active"
    wait_for(lambda: owner.is_observed(fid("py-e2e::charge")), "the observer on its host")
    frames = FrameCounter(owner)

    watcher.call({"function_id": fid("py-e2e::checkout"), "payload": 750})
    watcher.call({"function_id": fid("py-e2e::checkout"), "payload": 10})  # filtered out by input
    watcher.call({"function_id": fid("py-e2e::charge"), "payload": {"amount": 900}})  # no caller.function_id
    exactly(got, 1, "one activation")
    [activation] = got
    data = activation.data
    assert data["observer_id"] == handle.id
    assert (data["function_id"], data["namespace"], data["status"]) == (fid("py-e2e::charge"), "default", "success")
    assert data["input"] == {"amount": 750}
    assert data["output"] == {"charged": 750}
    assert data["caller"]["function_id"] == fid("py-e2e::checkout")
    assert "routed-watcher" in data["caller"]["worker_name"]
    assert all(isinstance(data[k], str) for k in ("activation_id", "invocation_id"))
    assert isinstance(data["started_at_ms"], int) and isinstance(data["duration_ms"], (int, float))
    assert activation.metadata == {"team": "payments"}  # the observer's metadata sidecar
    inv = activation.inv
    assert inv is not None
    assert inv.function_id == fid("py-e2e::on-big-charge")
    assert inv.observe_depth == 1
    assert inv.caller is not None and inv.caller["worker_name"] == "iii-engine"
    assert (inv.caller["function_id"], inv.caller["invocation_id"]) == (fid("py-e2e::charge"), data["invocation_id"])
    assert inv.activation == {"observer_id": handle.id, "activation_id": data["activation_id"]}
    # the engine evaluated the filters: the host's answers say nothing of observers, and nothing else went out
    charge = fid("py-e2e::charge")
    answered = [f for f in frames.frames if f.get("type") == "invocationresult" and f.get("function_id") == charge]
    assert len(answered) == 3 and not any("matched" in f or "delivered" in f for f in answered)
    assert "invocationsobserved" not in frames.types


def test_on_error_sees_failures_only_capture_limits_what_travels_deliver_input_sends_the_input(
    worker: Any, fid: Any
) -> None:
    owner = worker("routed-owner")
    watcher = worker("routed-watcher")

    def flaky(data: dict[str, Any]) -> int:
        if data["fail"]:
            raise RuntimeError("flaky failed")
        return data["n"]

    owner.register_function(fid("py-e2e::flaky"), flaky)
    errors = collector(watcher, fid("py-e2e::on-error"))
    inputs = collector(watcher, fid("py-e2e::on-input"))
    a = watcher.observe(
        {"function_id": fid("py-e2e::flaky"), "on": "error", "capture": ["error"]}, fid("py-e2e::on-error")
    )
    b = watcher.observe(
        {"function_id": fid("py-e2e::flaky"), "on": "complete", "deliver": "input"}, fid("py-e2e::on-input")
    )
    a.ready(timeout=10)
    b.ready(timeout=10)
    watcher.call({"function_id": fid("py-e2e::flaky"), "payload": {"fail": False, "n": 1}})
    with pytest.raises(Exception):
        watcher.call({"function_id": fid("py-e2e::flaky"), "payload": {"fail": True, "n": 2}})
    exactly(errors, 1, "the error activation")
    exactly(inputs, 2, "the input activations")
    assert errors[0].data["status"] == "error"
    assert errors[0].data["error"]["code"] == "invocation_failed"
    assert errors[0].data["error"]["message"] == "flaky failed"
    assert "input" not in errors[0].data
    assert sorted((x.data for x in inputs), key=lambda d: d["n"]) == [{"fail": False, "n": 1}, {"fail": True, "n": 2}]


def test_a_target_that_is_not_registered_keeps_the_observer_pending_until_it_is(worker: Any, fid: Any) -> None:
    owner = worker("routed-owner")
    watcher = worker("routed-watcher")
    owner.register_function(fid("py-e2e::tick"), lambda d: "ok")
    handle = watcher.observe({"function_id": fid("py-e2e::tick")}, fid("py-e2e::late-target"))
    settle_engine()  # the engine answered the observe and would have said anything else by now
    assert handle.state == "pending"
    got = collector(watcher, fid("py-e2e::late-target"))
    handle.ready(timeout=10)
    watcher.call({"function_id": fid("py-e2e::tick"), "payload": {}})
    exactly(got, 1, "the activation")


@pytest.mark.parametrize(
    ("code", "source", "target"),
    [
        ("SELF_TARGET", {"function_id": "py-e2e::self"}, "py-e2e::self"),
        ("INVALID_ARGS", {"function_id": "py-e2e::self", "args": {"expression": "* * *"}}, "py-e2e::other"),
        ("INVALID_FILTER", {"function_id": "py-e2e::self", "filters": {"input.op": "insert"}}, "py-e2e::other"),
    ],
)
def test_errors_turn_the_observer_to_error_and_ready_raises(
    worker: Any, fid: Any, code: str, source: Any, target: str
) -> None:
    owner = worker("routed-owner")
    owner.register_function(fid("py-e2e::self"), lambda d: None)
    owner.register_function(fid("py-e2e::other"), lambda d: None)
    handle = owner.observe({**source, "function_id": fid(source["function_id"])}, fid(target))
    with pytest.raises(ObserveRejection) as raised:
        handle.ready(timeout=10)
    assert raised.value.code == code
    assert handle.state == "error"
    assert handle.reason is not None and handle.reason["code"] == code


@pytest.mark.parametrize(
    ("code", "source_options", "target_options"),
    [
        ("NOT_OBSERVABLE", {"observable": False}, {}),  # D3
        ("FORBIDDEN", {}, {"private": True}),  # D1: a private function is never a target
    ],
)
def test_an_unobservable_function_or_a_private_target_turns_the_observer_to_error(
    worker: Any, fid: Any, code: str, source_options: dict[str, Any], target_options: dict[str, Any]
) -> None:
    owner = worker("routed-owner")
    watcher = worker("routed-watcher")
    owner.register_function(fid("py-e2e::source"), lambda d: d, **source_options)
    watcher.register_function(fid("py-e2e::target"), lambda d: None, **target_options)
    local_dispatch_ready(owner, fid("py-e2e::source"))
    local_dispatch_ready(watcher, fid("py-e2e::target"))
    handle = watcher.observe(fid("py-e2e::source"), fid("py-e2e::target"))
    with pytest.raises(ObserveRejection) as raised:
        handle.ready(timeout=10)
    assert raised.value.code == code
    assert handle.state == "error"
    assert owner.observers(fid("py-e2e::source")) == []


def test_dispose_stops_the_activations_and_max_fires_disposes_after_the_last_one(worker: Any, fid: Any) -> None:
    owner = worker("routed-owner")
    watcher = worker("routed-watcher")
    owner.register_function(fid("py-e2e::ping"), lambda d: "pong")
    disposed = collector(watcher, fid("py-e2e::on-ping-a"))
    once = collector(watcher, fid("py-e2e::on-ping-b"))
    a = watcher.observe({"function_id": fid("py-e2e::ping")}, fid("py-e2e::on-ping-a"))
    b = watcher.observe({"function_id": fid("py-e2e::ping")}, fid("py-e2e::on-ping-b"), {"max_fires": 1})
    a.ready(timeout=10)
    b.ready(timeout=10)
    a.dispose()
    assert a.state == "disposed"
    settle_engine()  # the unobserve reached the engine
    watcher.call({"function_id": fid("py-e2e::ping"), "payload": {}})
    watcher.call({"function_id": fid("py-e2e::ping"), "payload": {}})
    wait_for(lambda: b.state == "disposed", "max_fires to dispose the observer")
    settle_engine()
    assert disposed == []
    assert len(once) == 1
    assert b.reason is not None and b.reason["code"] == "MAX_FIRES"


def test_the_observer_survives_a_reconnect_of_its_worker(worker: Any, fid: Any) -> None:
    owner = worker("routed-owner")
    watcher = worker("routed-watcher")
    owner.register_function(fid("py-e2e::work"), lambda d: "done")
    got = collector(watcher, fid("py-e2e::on-work"))
    handle = watcher.observe({"function_id": fid("py-e2e::work")}, fid("py-e2e::on-work"))
    handle.ready(timeout=10)
    drop_connection(watcher)
    wait_for(lambda: watcher.get_connection_state() != "connected", "the disconnect")
    wait_for(lambda: watcher._registered_on_connection, "the reconnect", timeout=10)
    settle_engine()  # the replayed observe reached the engine
    owner.call({"function_id": fid("py-e2e::work"), "payload": {}})
    exactly(got, 1, "an activation after the reconnect")
    assert got[0].data["observer_id"] == handle.id
    assert handle.state == "active"


def test_expires_in_ms_an_expired_observer_is_disposed_and_fires_no_more(worker: Any, fid: Any) -> None:
    owner = worker("routed-owner")
    watcher = worker("routed-watcher")
    owner.register_function(fid("py-e2e::lease"), lambda d: "ok")
    got = collector(watcher, fid("py-e2e::on-lease"))
    handle = watcher.observe({"function_id": fid("py-e2e::lease")}, fid("py-e2e::on-lease"), {"expires_in_ms": 300})
    handle.ready(timeout=10)
    time.sleep(0.5)  # the lease (300 ms) runs out: time passing is the point here
    owner.call({"function_id": fid("py-e2e::lease"), "payload": {}})
    wait_for(lambda: handle.state == "disposed", "the expiry")
    settle_engine()
    assert got == []
    assert handle.reason is not None and handle.reason["code"] == "EXPIRED"


def test_the_loop_guard_observe_depth_travels_with_the_calls_a_target_makes(worker: Any, fid: Any) -> None:
    # py-e2e::loop is observed by py-e2e::on-loop, which calls py-e2e::loop
    # again: each hop is one observe level deeper (the SDK copies observe_depth
    # from the activation into the call), until the engine drops it.
    owner = worker("routed-owner")
    watcher = worker("routed-watcher")
    owner.register_function(fid("py-e2e::loop"), lambda n: n)
    depths: list[int] = []

    def on_loop(envelope: dict[str, Any]) -> None:
        inv = watcher.current_invocation()
        depths.append(inv.observe_depth if inv else -1)
        watcher.call({"function_id": fid("py-e2e::loop"), "payload": envelope["input"] + 1})

    watcher.register_function(fid("py-e2e::on-loop"), on_loop)
    handle = watcher.observe({"function_id": fid("py-e2e::loop"), "capture": ["input"]}, fid("py-e2e::on-loop"))
    handle.ready(timeout=10)
    watcher.call({"function_id": fid("py-e2e::loop"), "payload": 0})
    wait_for(lambda: len(depths) >= 8, "eight hops", timeout=10)

    def loop_guard_drops() -> int:
        info = watcher.call({"function_id": "engine::observers::get", "payload": {"id": handle.id}})
        info = info.get("observer", info) if isinstance(info, dict) else {}
        return int(info.get("dropped", {}).get("LOOP_GUARD", 0))

    # the chain is over once the engine dropped the ninth hop
    wait_for(lambda: loop_guard_drops() >= 1, "the loop guard to stop the chain", timeout=10)
    settle_engine()
    assert depths == [1, 2, 3, 4, 5, 6, 7, 8]


def test_a_match_hook_names_a_routed_calls_observers_and_the_engine_applies_their_filters(
    worker: Any, fid: Any
) -> None:
    """Decision 55: the answer to a routed call of a function with a match
    hook names the observers the hook matched, `[]` when none; the engine
    evaluates their `on` and filters against the input it forwarded and the
    answer."""
    owner = worker("routed-match-owner")
    watcher = worker("routed-match-watcher")
    fn = fid("py-e2e::row-changed")
    owner.on_observer_match(fn, observer=lambda obs: obs.args["table"], call=lambda call: call.input["table"])
    owner.register_function(
        fn,
        lambda change: {"rows": change["rows"]},
        args_format={"type": "object", "required": ["table"], "properties": {"table": {"type": "string"}}},
    )
    orders = collector(watcher, fid("py-e2e::on-orders"))
    big = collector(watcher, fid("py-e2e::on-big-orders"))
    users = collector(watcher, fid("py-e2e::on-users"))
    a = watcher.observe({"function_id": fn, "args": {"table": "orders"}}, fid("py-e2e::on-orders"))
    b = watcher.observe(
        {"function_id": fn, "args": {"table": "orders"}, "filters": {"output.rows": {"$gte": 10}}},
        fid("py-e2e::on-big-orders"),
    )
    c = watcher.observe({"function_id": fn, "args": {"table": "users"}}, fid("py-e2e::on-users"))
    for handle in (a, b, c):
        handle.ready(timeout=10)
    wait_for(lambda: len(owner.observers(fn)) == 3, "the observers on their host")
    frames = FrameCounter(owner)

    watcher.call({"function_id": fn, "payload": {"table": "orders", "rows": 3}})
    watcher.call({"function_id": fn, "payload": {"table": "orders", "rows": 30}})
    watcher.call({"function_id": fn, "payload": {"table": "audit", "rows": 99}})
    exactly(orders, 2, "both orders calls")
    exactly(big, 1, "the big orders call")
    exactly(users, 0, "no users call")
    assert big[0].data["output"] == {"rows": 30} and big[0].data["observer_id"] == b.id
    answered = [f for f in frames.frames if f.get("type") == "invocationresult" and f.get("function_id") == fn]
    assert [sorted(f["matched"]) for f in answered] == [sorted([a.id, b.id]), sorted([a.id, b.id]), []]
    assert not any("delivered" in f for f in answered)
    assert "invocationsobserved" not in frames.types


def test_a_routed_call_its_deliver_hook_delivered_activates_no_observer(worker: Any, fid: Any) -> None:
    """Decision 55: the answer of a function whose deliver hook delivered the
    call says `delivered: true`, and the engine activates no observer for
    it: the target runs once, from the hook's call through the observer."""
    owner = worker("routed-deliver-owner")
    watcher = worker("routed-deliver-watcher")
    fn = fid("py-e2e::request")

    @owner.on_observer_deliver(fn)
    async def route(call: Any, observers: Any) -> Any:
        [obs] = observers
        return await owner.call_async({"observer": obs, "payload": {"routed": call.input}})

    owner.register_function(fn, lambda request: request)
    got = collector(watcher, fid("py-e2e::handler"))
    handle = watcher.observe({"function_id": fn}, fid("py-e2e::handler"))
    handle.ready(timeout=10)
    wait_for(lambda: owner.observers(fn), "the observer on its host")
    frames = FrameCounter(owner)

    assert watcher.call({"function_id": fn, "payload": {"path": "/x"}}) is None  # what the target returned
    exactly(got, 1, "the call through the observer and nothing else")
    data = got[0].data  # the payload the hook sent (with the engine's caller stamps), not an envelope
    assert "observer_id" not in data and data["routed"]["path"] == "/x"
    [answer] = [f for f in frames.frames if f.get("type") == "invocationresult" and f.get("function_id") == fn]
    assert answer["delivered"] is True and "matched" not in answer
