"""The local path: when ``call()`` runs in-process, the invocation context
and caller propagation, value semantics, and the reports of what local calls
delivered. Against a fake engine."""

from __future__ import annotations

import asyncio
import threading
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
    reported_items,
    settle,
    wait_until,
)
from pydantic import BaseModel

from iii import Call, CallAction, InvocationError, current_invocation
from iii.iii import III
from iii.observable import same_data


def _confirm(
    client: III,
    engine: FakeEngine,
    function_id: str,
    observers: list[dict[str, Any]] | None = None,
    namespace: str = "default",
    local_dispatch: bool = True,
) -> None:
    engine.deliver(function_registered(function_id, observers, namespace=namespace, local_dispatch=local_dispatch))
    state = client._functions[function_id].observe
    wait_until(lambda: state.confirmed, what=f"functionregistered for {function_id}")
    if observers:
        wait_until(lambda: len(state.active.observers) == len(observers), what="the observers")
        # The acks go out after the filters compile; wait for them so a test that clears the frames next does not
        # see them arrive late.
        ids = {o["id"] for o in observers}

        def acked() -> set[str]:
            return {f.get("id") for f in engine.frames_of("observerack") if f.get("function_id") == function_id}

        wait_until(lambda: ids <= acked(), what=f"the observers' acks for {function_id}")


def _invocations(engine: FakeEngine) -> list[dict[str, Any]]:
    return [f for f in engine.frames_of("invokefunction") if f.get("function_id") != "engine::workers::register"]


# ── when call() runs in-process ─────────────────────────────────────────


def test_a_confirmed_function_runs_in_process_and_puts_nothing_on_the_wire(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    threads: list[threading.Thread] = []

    def add(data: dict[str, int]) -> int:
        threads.append(threading.current_thread())
        return data["a"] + data["b"]

    client.register_function("math::add", add)
    # before functionregistered: the engine path
    engine.on_frame = lambda f: (
        [
            {
                "type": "invocationresult",
                "invocation_id": f["invocation_id"],
                "function_id": f["function_id"],
                "result": 0,
            }
        ]
        if f.get("type") == "invokefunction" and f.get("invocation_id")
        else None
    )
    assert client.call({"function_id": "math::add", "payload": {"a": 1, "b": 2}}) == 0
    assert len(_invocations(engine)) == 1

    _confirm(client, engine, "math::add")
    engine.clear()
    assert client.call({"function_id": "math::add", "payload": {"a": 1, "b": 2}}) == 3
    # a sync handler runs on a thread of its own (so the caller can stop waiting), never on the SDK loop
    assert threads[-1] is not threading.current_thread()
    assert threads[-1] is not client._thread

    async def from_a_loop() -> int:
        return await client.call_async({"function_id": "math::add", "payload": {"a": 2, "b": 2}})

    assert asyncio.run(from_a_loop()) == 4
    assert threads[-1] is not threading.current_thread()  # on a thread of its own: the loop keeps running
    settle(client, engine)
    assert engine.frames() == []

    # dispatch="remote" and enqueue always go to the engine
    assert client.call({"function_id": "math::add", "payload": {"a": 1, "b": 2}, "dispatch": "remote"}) == 0
    client.call({"function_id": "math::add", "payload": {}, "action": CallAction.Enqueue(queue="q")})
    assert len(_invocations(engine)) == 2
    with pytest.raises(ValueError):
        client.call({"function_id": "math::add", "payload": {}, "dispatch": "elsewhere"})


def test_an_async_handler_runs_on_the_calling_loop(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    loops: list[asyncio.AbstractEventLoop] = []

    async def handler(data: Any) -> Any:
        loops.append(asyncio.get_running_loop())
        return data

    client.register_function("loop::fn", handler)
    _confirm(client, engine, "loop::fn")

    async def caller() -> Any:
        result = await client.call_async({"function_id": "loop::fn", "payload": 1})
        return result, asyncio.get_running_loop()

    result, loop = asyncio.run(caller())
    assert result == 1
    assert loops[-1] is loop  # not the SDK loop
    # from a thread (sync call): on the SDK loop
    assert client.call({"function_id": "loop::fn", "payload": 2}) == 2
    assert loops[-1] is client._loop


def test_stays_local_for_the_namespace_the_function_registered_in(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine, namespace="orders")
    client.register_function("ns::fn", lambda d: "local")
    _confirm(client, engine, "ns::fn", namespace="orders")
    assert client.call({"function_id": "ns::fn", "payload": None}) == "local"
    assert client.call({"function_id": "ns::fn", "payload": None, "namespace": "orders"}) == "local"
    engine.clear()
    engine.on_frame = lambda f: (
        [
            {
                "type": "invocationresult",
                "invocation_id": f["invocation_id"],
                "function_id": f["function_id"],
                "result": "remote",
            }
        ]
        if f.get("type") == "invokefunction" and f.get("invocation_id")
        else None
    )
    assert client.call({"function_id": "ns::fn", "payload": None, "namespace": "default"}) == "remote"


def test_never_runs_an_http_invoked_function_locally(engine: FakeEngine, connect: Any) -> None:
    from iii_helpers.http import HttpInvocationConfig

    client = connect(engine)
    client.register_function("ext::fn", HttpInvocationConfig(url="http://127.0.0.1:9/fn"))
    _confirm(client, engine, "ext::fn")
    assert client._functions["ext::fn"].observe.local_dispatch is False
    client.call({"function_id": "ext::fn", "payload": {}, "action": CallAction.Void()})
    engine.wait_for(lambda: _invocations(engine), what="the routed call")


def test_local_dispatch_false_and_an_unconfirmed_function_go_through_the_engine(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    client.register_function("opt::out", lambda d: d, local_dispatch=False)
    client.register_function("eng::off", lambda d: d)
    _confirm(client, engine, "opt::out")
    _confirm(client, engine, "eng::off", local_dispatch=False)
    for fn in ("opt::out", "eng::off"):
        client.call({"function_id": fn, "payload": {}, "action": CallAction.Void()})
    engine.wait_for(lambda: len(_invocations(engine)) == 2, what="two routed calls")


def test_void_runs_locally_fire_and_forget(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    done = threading.Event()
    got: list[Any] = []

    returned = threading.Event()

    def blocked(data: Any) -> None:
        # runs to its end only once the caller got its answer
        returned.wait(2)
        got.append(("handler finished", data))
        done.set()

    client.register_function("void::fn", blocked)
    _confirm(client, engine, "void::fn")
    assert client.call({"function_id": "void::fn", "payload": 1, "action": CallAction.Void()}) is None
    got.append(("caller returned", None))
    returned.set()
    assert done.wait(2)
    assert got == [("caller returned", None), ("handler finished", 1)]
    assert _invocations(engine) == []


def test_local_dispatch_keeps_serving_while_disconnected(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    client.register_function("still::here", lambda d: d * 2)
    _confirm(client, engine, "still::here")
    engine.stop()
    wait_until(lambda: client.get_connection_state() != "connected", what="the disconnect")
    assert client.call({"function_id": "still::here", "payload": 21}) == 42


# ── value semantics, errors and timeouts: as on the remote path ────────────


def test_a_handler_that_mutates_its_input_does_not_affect_the_caller_and_the_reverse(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    kept: dict[str, Any] = {}

    def mutate(data: dict[str, Any]) -> dict[str, Any]:
        data["items"].append("handler")
        kept["out"] = {"nested": {"n": 1}}
        return kept["out"]

    client.register_function("value::fn", mutate)
    _confirm(client, engine, "value::fn")
    payload = {"items": ["caller"]}
    result = client.call({"function_id": "value::fn", "payload": payload})
    assert payload == {"items": ["caller"]}
    result["nested"]["n"] = 99
    assert kept["out"] == {"nested": {"n": 1}}

    async def from_a_loop() -> None:
        p = {"items": []}
        r = await client.call_async({"function_id": "value::fn", "payload": p})
        assert p == {"items": []}
        r["nested"]["n"] = 5
        assert kept["out"] == {"nested": {"n": 1}}

    asyncio.run(from_a_loop())


def test_values_deepcopy_cannot_copy_fall_back_to_their_json_form(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)

    class Model(BaseModel):
        name: str

    class Uncopyable:
        def __deepcopy__(self, memo: Any) -> Any:
            raise TypeError("no copies")

    seen: list[Any] = []
    client.register_function("copy::fn", lambda data: seen.append(data) or "ok")
    _confirm(client, engine, "copy::fn")
    model = Model(name="a")
    client.call({"function_id": "copy::fn", "payload": model})
    assert seen[-1] == model and seen[-1] is not model  # deep copy keeps the model
    lock = threading.Lock()  # deepcopy refuses locks; JSON leaves them out
    client.call({"function_id": "copy::fn", "payload": {"lock": lock, "n": 2}})
    assert seen[-1] == {"n": 2}
    client.call({"function_id": "copy::fn", "payload": {"m": model, "u": Uncopyable(), "n": 1}})
    assert seen[-1]["m"] == {"name": "a"} and seen[-1]["n"] == 1  # the JSON form


def test_metadata_is_any_json_value_and_none_is_no_metadata_local_routed_and_received(
    engine: FakeEngine, connect: Any
) -> None:
    """Decision 54: ``None`` is no metadata on every path; every other JSON value passes as given."""
    client = connect(engine)
    seen: list[Any] = []

    def handler(data: Any, metadata: Any = "absent") -> None:
        seen.append(metadata)

    client.register_function("meta::fn", handler)
    _confirm(client, engine, "meta::fn")
    engine.clear()
    values: list[Any] = [None, ["a", 1], "tag", 0, False, {"tenant": "acme"}]
    # local: the handler gets None for None, every other value as given
    for metadata in values:
        client.call({"function_id": "meta::fn", "payload": {}, "metadata": metadata})
    assert seen == values
    assert _invocations(engine) == []
    # routed: None is left off the frame
    for metadata in values:
        request = {"function_id": "meta::fn", "payload": {}, "metadata": metadata, "dispatch": "remote"}
        client.call({**request, "action": CallAction.Void()})
    sent = engine.wait_for(lambda: len(_invocations(engine)) == len(values) and _invocations(engine), what="calls")
    assert [f.get("metadata", "absent") for f in sent] == ["absent", *values[1:]]
    # received: an invokefunction with metadata null reaches the handler as None
    seen.clear()
    for i, metadata in enumerate(values):
        engine.deliver(invoke("meta::fn", {}, f"in-{i}", metadata=metadata))
        engine.wait_for(lambda i=i: answers(engine, f"in-{i}"), what="the answer")
    assert seen == values


def test_a_failing_handler_raises_the_invocation_error_the_remote_path_gives(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)

    def boom(data: Any) -> None:
        raise ValueError("out of stock")

    async def aboom(data: Any) -> None:
        raise ValueError("out of stock")

    client.register_function("fail::sync", boom)
    client.register_function("fail::async", aboom)
    _confirm(client, engine, "fail::sync")
    _confirm(client, engine, "fail::async")
    for fn in ("fail::sync", "fail::async"):
        with pytest.raises(InvocationError) as raised:
            client.call({"function_id": fn, "payload": {}})
        assert raised.value.code == "invocation_failed"
        assert raised.value.message == "out of stock"
        assert raised.value.function_id == fn
        assert "ValueError: out of stock" in (raised.value.stacktrace or "")
        assert isinstance(raised.value.__cause__, ValueError)


def test_times_out_like_the_remote_path_and_the_handler_runs_to_its_end(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    events: list[str] = []
    release = threading.Event()

    async def blocked_async(data: Any) -> str:
        while not release.is_set():
            await asyncio.sleep(0.005)
        events.append("async handler finished")
        return "late"

    def blocked_sync(data: Any) -> str:
        release.wait(5)  # never returns on its own within the call's timeout
        events.append("sync handler finished")
        return "late"

    client.register_function("slow::async", blocked_async)
    client.register_function("slow::sync", blocked_sync)
    _confirm(client, engine, "slow::async")
    _confirm(client, engine, "slow::sync")
    for fn in ("slow::async", "slow::sync"):
        with pytest.raises(InvocationError) as raised:
            client.call({"function_id": fn, "payload": {}, "timeout_ms": 50})
        assert raised.value.code == "TIMEOUT"
        events.append(f"{fn} timed out")

    async def from_a_loop() -> None:
        with pytest.raises(InvocationError) as raised:
            await client.call_async({"function_id": "slow::sync", "payload": {}, "timeout_ms": 50})
        assert raised.value.code == "TIMEOUT"
        events.append("async caller timed out")

    asyncio.run(from_a_loop())
    # the callers stopped waiting while every handler was still running
    assert events == ["slow::async timed out", "slow::sync timed out", "async caller timed out"]
    release.set()
    wait_until(lambda: len(events) == 6, what="the handlers to finish")
    assert sorted(events[3:]) == ["async handler finished", "sync handler finished", "sync handler finished"]
    # their outcomes still count as local invocations (the one started from the
    # loop asyncio.run closed is gone with that loop)
    stats = client._reporter._counters.stats
    wait_until(lambda: "slow::sync" in stats and "slow::async" in stats, what="the counts")
    assert (stats["slow::async"].count, stats["slow::sync"].count) == (1, 1)


def test_no_message_size_limit_on_the_local_path(engine: FakeEngine, connect: Any) -> None:
    from iii.iii import _MAX_JSON_OUTBOUND_BYTES

    client = connect(engine)
    client.register_function("big::fn", lambda data: len(data))
    _confirm(client, engine, "big::fn")
    # Above the outbound message limit (64 MiB): smaller payloads go remote
    # as fragmented messages, so only this size is refused locally.
    big = "x" * (_MAX_JSON_OUTBOUND_BYTES + 1)
    assert client.call({"function_id": "big::fn", "payload": big}) == len(big)
    with pytest.raises(InvocationError) as raised:
        client.call({"function_id": "big::fn", "payload": big, "dispatch": "remote"})
    assert raised.value.code == "payload_too_large"


def test_a_deep_chain_of_local_calls_falls_back_to_the_engine(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    engine.on_frame = lambda f: (
        [
            {
                "type": "invocationresult",
                "invocation_id": f["invocation_id"],
                "function_id": f["function_id"],
                "result": -1,
            }
        ]
        if f.get("type") == "invokefunction" and f.get("invocation_id")
        else None
    )

    def recurse(n: int) -> int:
        return n if n == 0 else client.call({"function_id": "rec::fn", "payload": n - 1})

    client.register_function("rec::fn", recurse)
    _confirm(client, engine, "rec::fn")
    assert client.call({"function_id": "rec::fn", "payload": 63}) == 0  # 64 nested local calls
    assert _invocations(engine) == []
    assert client.call({"function_id": "rec::fn", "payload": 64}) == -1  # the 65th goes remote
    assert len(_invocations(engine)) == 1


# ── the invocation context and caller propagation ──────────────────────────


def test_current_invocation_in_a_local_handler_and_the_caller_chain_of_nested_local_calls(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    seen: dict[str, Any] = {}

    def inner(data: Any) -> None:
        seen["inner"] = client.current_invocation()

    async def outer(data: Any) -> None:
        seen["outer"] = current_invocation()
        await client.call_async({"function_id": "ctx::inner", "payload": {}})

    client.register_function("ctx::inner", inner)
    client.register_function("ctx::outer", outer)
    _confirm(client, engine, "ctx::inner")
    _confirm(client, engine, "ctx::outer")
    assert client.current_invocation() is None
    client.call({"function_id": "ctx::outer", "payload": {}})
    out, inn = seen["outer"], seen["inner"]
    assert out.function_id == "ctx::outer"
    assert out.caller == {"worker_id": "worker-1", "worker_name": "observe-test"}
    assert out.observe_depth == 0
    assert inn.function_id == "ctx::inner"
    assert inn.caller == {
        "worker_id": "worker-1",
        "worker_name": "observe-test",
        "function_id": "ctx::outer",
        "invocation_id": out.invocation_id,
        "parent": {"worker_id": "worker-1", "worker_name": "observe-test"},
    }
    assert client.current_invocation() is None


def test_an_inbound_activation_sets_the_context_is_answered_and_its_calls_carry_caller_and_depth(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    seen: dict[str, Any] = {}

    def target(envelope: dict[str, Any]) -> str:
        # a sync handler: its own thread, started in a copy of the context
        seen["inv"] = client.current_invocation()
        seen["thread"] = threading.current_thread()
        client.call({"function_id": "elsewhere::fn", "payload": {}, "action": CallAction.Void()})
        return "done"

    client.register_function("alerts::on-fail", target)
    engine.deliver(
        {
            "type": "invokefunction",
            "invocation_id": "act-1",
            "function_id": "alerts::on-fail",
            "data": {"observer_id": "o1"},
            "metadata": {"team": "oncall"},
            "caller": {"worker_name": "iii-engine", "function_id": "payments::charge", "invocation_id": "obs-inv"},
            "observe_depth": 1,
            "activation": {"observer_id": "o1", "activation_id": "a1"},
        }
    )
    engine.wait_for(lambda: engine.frames_of("invocationresult"), what="the answer")
    result = engine.frames_of("invocationresult")[0]
    assert (result["invocation_id"], result["result"]) == ("act-1", "done")  # answered like any call (D16)
    inv = seen["inv"]
    assert inv.function_id == "alerts::on-fail"
    assert inv.invocation_id == "act-1"
    assert inv.observe_depth == 1
    assert inv.activation == {"observer_id": "o1", "activation_id": "a1"}
    assert inv.caller == {"worker_name": "iii-engine", "function_id": "payments::charge", "invocation_id": "obs-inv"}
    assert seen["thread"] is not client._thread
    call = engine.wait_for(
        lambda: [f for f in _invocations(engine) if f["function_id"] == "elsewhere::fn"], what="the outgoing call"
    )[0]
    assert call["caller"] == {
        "function_id": "alerts::on-fail",
        "invocation_id": "act-1",
        "parent": {"worker_name": "iii-engine", "function_id": "payments::charge", "invocation_id": "obs-inv"},
    }
    assert call["observe_depth"] == 1


def test_omits_observe_depth_when_0_and_caller_outside_any_handler(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    client.register_function(
        "app::fn", lambda d: client.call({"function_id": "x::y", "payload": {}, "action": CallAction.Void()})
    )
    client.call({"function_id": "outside::fn", "payload": {}, "action": CallAction.Void()})
    engine.deliver({"type": "invokefunction", "invocation_id": "i-1", "function_id": "app::fn", "data": {}})
    engine.wait_for(lambda: len(_invocations(engine)) == 2, what="both calls")
    outside, inside = _invocations(engine)
    assert "caller" not in outside and "observe_depth" not in outside
    assert inside["caller"] == {"function_id": "app::fn", "invocation_id": "i-1"}
    assert "observe_depth" not in inside


def test_sends_no_caller_to_an_engine_without_observe(engine_factory: Any, connect: Any) -> None:
    legacy = engine_factory("legacy")
    client = connect(legacy)
    client.register_function(
        "app::fn", lambda d: client.call({"function_id": "x::y", "payload": {}, "action": CallAction.Void()})
    )
    legacy.deliver({"type": "invokefunction", "invocation_id": "i-1", "function_id": "app::fn", "data": {}})
    call = legacy.wait_for(
        lambda: [f for f in legacy.frames_of("invokefunction") if f["function_id"] == "x::y"], what="the call"
    )[0]
    assert "caller" not in call and "observe_depth" not in call


def test_sync_handlers_keep_the_opentelemetry_context_on_their_thread(engine: FakeEngine, connect: Any) -> None:
    from opentelemetry import context as otel_context

    client = connect(engine)
    key = otel_context.create_key("observe-test")
    seen: list[Any] = []
    client.register_function("otel::fn", lambda d: seen.append(otel_context.get_value(key)))
    _confirm(client, engine, "otel::fn")

    async def caller() -> None:
        token = otel_context.attach(otel_context.set_value(key, "carried"))
        try:
            await client.call_async({"function_id": "otel::fn", "payload": {}})
        finally:
            otel_context.detach(token)

    asyncio.run(caller())
    assert seen == ["carried"]


# ── delivery: local invocations are evaluated and reported ─────────────────


def test_reports_a_matched_call_once_with_the_union_of_the_matched_captures(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    client.register_function("local::price", lambda d: {"total": d["qty"] * 10})
    _confirm(
        client,
        engine,
        "local::price",
        [
            observer_info("a", "t", capture=["input"], filters={"$and": [{"input.qty": {"$gte": 2}}]}),
            observer_info("b", "t", capture=["output", "metadata"]),
            observer_info("c", "t", capture=["error"], filters={"$and": [{"input.qty": {"$gt": 100}}]}),
        ],
    )
    engine.clear()
    payload = {"qty": 3}
    client.call({"function_id": "local::price", "payload": payload, "metadata": {"channel": "web"}})
    settle(client, engine)
    [item] = reported_items(engine)
    assert item["matched"] == ["a", "b"]
    assert item["status"] == "success"
    assert item["input"] == {"qty": 3}
    assert item["output"] == {"total": 30}
    assert item["metadata"] == {"channel": "web"}
    assert "caller" not in item and "error" not in item
    assert item["observe_depth"] == 0
    assert isinstance(item["duration_ms"], (int, float))
    assert _invocations(engine) == []


def test_capture_empty_reports_ids_status_and_timing_without_a_payload(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)

    def checkout(data: dict[str, Any]) -> dict[str, Any]:
        return client.call({"function_id": "local::price", "payload": data, "metadata": {"channel": "web"}})

    client.register_function("local::price", lambda d: {"total": d["qty"] * 10})
    client.register_function("local::checkout", checkout)
    _confirm(client, engine, "local::price", [observer_info("bare", "t", capture=[])])
    _confirm(client, engine, "local::checkout")
    engine.clear()
    # made from a handler, so the call has a caller too
    assert client.call({"function_id": "local::checkout", "payload": {"qty": 3}}) == {"total": 30}
    settle(client, engine)
    [item] = reported_items(engine)
    assert not {"input", "output", "output_is_input", "error", "caller", "metadata"} & item.keys()
    assert {
        "function_id",
        "namespace",
        "invocation_id",
        "matched",
        "status",
        "started_at_ms",
        "duration_ms",
        "observe_depth",
    } <= item.keys()
    assert (item["function_id"], item["matched"], item["status"]) == ("local::price", ["bare"], "success")
    assert isinstance(item["invocation_id"], str)
    assert isinstance(item["started_at_ms"], int) and isinstance(item["duration_ms"], (int, float))


def test_on_error_sees_only_failures_with_the_error_and_the_caller(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)

    def flaky(data: dict[str, Any]) -> int:
        if data["fail"]:
            raise RuntimeError("flaky failed")
        return 1

    client.register_function("local::flaky", flaky)
    _confirm(client, engine, "local::flaky", [observer_info("e", "t", on="error", capture=["error", "caller"])])
    client.call({"function_id": "local::flaky", "payload": {"fail": False}})
    with pytest.raises(InvocationError):
        client.call({"function_id": "local::flaky", "payload": {"fail": True}})
    settle(client, engine)
    [item] = reported_items(engine)
    assert item["status"] == "error"
    assert item["error"]["code"] == "invocation_failed"
    assert item["error"]["message"] == "flaky failed"
    # made outside any handler: no caller (the engine stamps this worker's on every item)
    assert "caller" not in item


@pytest.mark.parametrize("hook", [False, True])
def test_the_reported_input_and_metadata_are_the_callers_at_call_start(
    engine: FakeEngine, connect: Any, hook: bool
) -> None:
    client = connect(engine)
    delivered: list[tuple[Any, Any]] = []

    def mutate(data: dict[str, Any], metadata: dict[str, Any]) -> dict[str, Any]:
        data["touched"] = True
        metadata["touched"] = True
        return data

    if hook:

        @client.on_observer_deliver("local::mutate")
        def keep(call: Call[Any, Any], observers: Any) -> Any:
            delivered.append((call.input, call.metadata))
            return call.output

    client.register_function("local::mutate", mutate)
    _confirm(client, engine, "local::mutate", [observer_info("o", "t", capture=["input", "output", "metadata"])])
    payload = {"n": 1}
    metadata = {"tenant": "acme"}
    result = client.call({"function_id": "local::mutate", "payload": payload, "metadata": metadata})
    # the handler works on its own copies; the caller keeps its values
    assert result == {"n": 1, "touched": True}
    assert payload == {"n": 1} and metadata == {"tenant": "acme"}
    if hook:
        assert delivered == [({"n": 1}, {"tenant": "acme"})]
        return
    [item] = engine.wait_for(lambda: reported_items(engine), what="the report")
    # the payload and metadata as the caller passed them, apart from what the handler did to its copies
    assert (item["input"], item["metadata"]) == ({"n": 1}, {"tenant": "acme"})
    assert item["output"] == {"n": 1, "touched": True} and "output_is_input" not in item


def test_a_non_private_handler_that_returns_its_own_input_unchanged_reports_it_once(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    client.register_function("echo::fn", lambda data: data)
    _confirm(client, engine, "echo::fn", [observer_info("o", "audit::echo")])
    engine.clear()
    payload = {"n": 3, "tags": ["a"]}
    out = client.call({"function_id": "echo::fn", "payload": payload})
    assert out == payload and out is not payload
    [item] = engine.wait_for(lambda: reported_items(engine), what="the report")
    assert item["input"] == {"n": 3, "tags": ["a"]}
    assert item.get("output_is_input") is True and "output" not in item


def _bump(data: dict[str, Any]) -> dict[str, Any]:
    data["n"] += 1
    return data


@pytest.mark.parametrize(
    ("handler", "output"),
    [
        pytest.param(_bump, {"n": 4}, id="changes its input before returning it"),
        pytest.param(lambda data: dict(data), {"n": 3}, id="returns an equal object of its own"),
    ],
)
def test_a_non_private_handler_that_does_not_return_its_input_unchanged_reports_both(
    engine: FakeEngine, connect: Any, handler: Any, output: dict[str, Any]
) -> None:
    client = connect(engine)
    client.register_function("echo::fn", handler)
    _confirm(client, engine, "echo::fn", [observer_info("o", "audit::echo")])
    engine.clear()
    assert client.call({"function_id": "echo::fn", "payload": {"n": 3}}) == output
    [item] = engine.wait_for(lambda: reported_items(engine), what="the report")
    # the input as the call started
    assert (item["input"], item["output"]) == ({"n": 3}, output)
    assert "output_is_input" not in item


def test_same_data_compares_json_data_in_one_walk() -> None:
    assert same_data({"a": [1, {"b": None}], "n": float("nan")}, {"n": float("nan"), "a": [1, {"b": None}]})
    assert not same_data({"a": [1]}, {"a": [1, 2]})
    assert not same_data({"a": 1}, {"a": True})
    assert not same_data({"a": 1}, {"a": 1.0})
    assert not same_data({"at": object()}, {"at": object()})
    cycle: dict[str, Any] = {}
    cycle["self"] = cycle
    other: dict[str, Any] = {}
    other["self"] = other
    assert not same_data(cycle, other)
    shared = {"n": 1}
    assert not same_data({"x": shared, "y": shared}, {"x": {"n": 1}, "y": {"n": 1}})


def test_reports_nothing_for_a_call_no_filter_matches_and_nothing_for_an_unobserved_function(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    client.register_function("local::quiet", lambda d: d)
    client.register_function("local::unobserved", lambda d: d)
    _confirm(client, engine, "local::quiet", [observer_info("o", "t", filters={"$and": [{"input": {"$eq": "never"}}]})])
    _confirm(client, engine, "local::unobserved")
    engine.clear()
    for _ in range(10):
        client.call({"function_id": "local::quiet", "payload": "x"})
        client.call({"function_id": "local::unobserved", "payload": "x"})
    settle(client, engine)
    assert engine.frames() == []


def test_a_call_started_before_the_observer_was_added_is_not_reported(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    release = threading.Event()
    entered = threading.Event()

    def wait(data: Any) -> None:
        entered.set()
        release.wait(2)

    client.register_function("local::race", wait)
    _confirm(client, engine, "local::race")
    runner = threading.Thread(target=lambda: client.call({"function_id": "local::race", "payload": 1}))
    runner.start()
    assert entered.wait(2)
    engine.deliver(observer_added("local::race", 1, observer_info("late", "t")))
    engine.wait_for(lambda: acks(engine), what="the ack")
    release.set()
    runner.join(2)
    settle(client, engine)
    assert reported_items(engine) == []  # evaluated against the set captured at call start
    client.call({"function_id": "local::race", "payload": 2})
    engine.wait_for(lambda: reported_items(engine), what="the next call's report")


def test_zero_wire_unobserved_local_calls_send_zero_frames_with_a_positive_control(
    engine: FakeEngine, connect: Any
) -> None:
    # sdk.md §7 asks for 1M; that takes minutes in CI (pytest --cov --cov-branch), so
    # 20 000, with the positive control below showing the counter sees frames.
    calls = 20_000
    client = connect(engine)
    client.register_function("zw::inc", lambda n: n + 1)
    client.register_function("zw::observed", lambda n: n)
    _confirm(client, engine, "zw::inc")
    _confirm(client, engine, "zw::observed", [observer_info("o", "t", capture=["input"])])
    sent: list[str] = []
    ws = client._ws
    assert ws is not None
    real_send = ws.send

    async def counting_send(data: Any, *args: Any, **kwargs: Any) -> None:
        sent.append(str(data))  # counted at the socket write, on the SDK loop, while the loop runs
        await real_send(data, *args, **kwargs)

    ws.send = counting_send  # type: ignore[method-assign]
    for i in range(calls):
        client.call({"function_id": "zw::inc", "payload": i})
    settle(client, engine)
    per_call = [d for d in sent if '"localinvocationstats"' not in d and "barrier" not in d]
    assert per_call == []
    # positive control: an observed function in the same harness does reach the socket
    client.call({"function_id": "zw::observed", "payload": 1})
    wait_until(lambda: any('"invocationsobserved"' in d for d in sent), what="the control's report")


def test_counts_local_invocations_in_localinvocationstats_only_when_non_zero(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)

    def maybe(data: bool) -> None:
        if data:
            raise RuntimeError("no")

    client.register_function("stats::fn", maybe)
    _confirm(client, engine, "stats::fn")
    engine.clear()
    assert client._reporter.send_stats() is False  # nothing yet
    client.call({"function_id": "stats::fn", "payload": False})
    with pytest.raises(InvocationError):
        client.call({"function_id": "stats::fn", "payload": True})

    async def send() -> bool:
        return client._reporter.send_stats()

    assert client._run_on_loop(send()) is True
    [stats] = engine.wait_for(lambda: engine.frames_of("localinvocationstats"), what="the stats")
    assert stats["functions"][0]["function_id"] == "stats::fn"
    assert (stats["functions"][0]["count"], stats["functions"][0]["errors"]) == (2, 1)
    assert stats["dropped_reports"] == []
    assert client._run_on_loop(send()) is False  # counters reset


def test_a_local_calls_report_uses_the_metadata_snapshot_taken_at_call_start(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    release = threading.Event()
    client.register_function("snap::fn", lambda data: release.wait(2) and None)
    _confirm(client, engine, "snap::fn", [observer_info("o", "t", capture=["input", "metadata"])])
    payload = {"n": 1}
    metadata = {"tag": "original"}
    client.call({"function_id": "snap::fn", "payload": payload, "metadata": metadata, "action": CallAction.Void()})
    # the caller changes its objects while the call is still running
    payload["n"] = 2
    metadata["tag"] = "changed"
    release.set()
    engine.wait_for(lambda: reported_items(engine), what="the report")
    [item] = reported_items(engine)
    assert item["input"] == {"n": 1}
    assert item["metadata"] == {"tag": "original"}


def test_a_sync_local_handler_raising_a_base_exception_is_counted_and_reported(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)

    def exits(data: Any) -> None:
        raise SystemExit("handler exited")

    client.register_function("exit::fn", exits)
    _confirm(client, engine, "exit::fn", [observer_info("o", "t", on="error", capture=["error"])])
    with pytest.raises(InvocationError) as raised:
        client.call({"function_id": "exit::fn", "payload": {}})
    assert raised.value.code == "invocation_failed"
    assert isinstance(raised.value.__cause__, SystemExit)
    stats = client._reporter._counters.stats["exit::fn"]
    assert (stats.count, stats.errors) == (1, 1)
    engine.wait_for(lambda: reported_items(engine), what="the report")
    assert reported_items(engine)[0]["status"] == "error"


def test_an_async_handler_raising_a_base_exception_fails_the_call_and_the_sdk_loop_keeps_running(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)

    async def exits(data: Any) -> None:
        raise SystemExit("async handler exited")

    client.register_function("aexit::fn", exits)
    client.register_function("alive::fn", lambda d: "alive")
    _confirm(client, engine, "aexit::fn")
    _confirm(client, engine, "alive::fn")
    with pytest.raises(InvocationError) as raised:
        client.call({"function_id": "aexit::fn", "payload": {}})
    assert raised.value.code == "invocation_failed"
    assert isinstance(raised.value.__cause__, SystemExit)

    async def from_a_loop() -> None:
        with pytest.raises(InvocationError) as raised:
            await client.call_async({"function_id": "aexit::fn", "payload": {}})
        assert isinstance(raised.value.__cause__, SystemExit)

    asyncio.run(from_a_loop())
    stats = client._reporter._counters.stats["aexit::fn"]
    assert (stats.count, stats.errors) == (2, 2)
    # the SDK loop survived: local and remote work goes on
    assert client._thread.is_alive()
    assert client.call({"function_id": "alive::fn", "payload": {}}) == "alive"
    # a remote invocation of it is answered with an error, not a dead loop
    engine.deliver({"type": "invokefunction", "invocation_id": "r-1", "function_id": "aexit::fn", "data": {}})
    [answer] = engine.wait_for(lambda: engine.frames_of("invocationresult"), what="the answer")
    assert answer["error"]["code"] == "invocation_failed"
    assert "SystemExit" in answer["error"]["message"]
    assert client.call({"function_id": "alive::fn", "payload": {}}) == "alive"


def test_a_void_local_call_that_raises_a_base_exception_is_logged(
    engine: FakeEngine, connect: Any, caplog: pytest.LogCaptureFixture
) -> None:
    import logging

    client = connect(engine)

    def sync_exits(data: Any) -> None:
        raise SystemExit("sync void exited")

    async def async_exits(data: Any) -> None:
        raise KeyboardInterrupt("async void interrupted")

    client.register_function("vexit::sync", sync_exits)
    client.register_function("vexit::async", async_exits)
    _confirm(client, engine, "vexit::sync")
    _confirm(client, engine, "vexit::async")
    with caplog.at_level(logging.ERROR, logger="iii.iii"):
        assert client.call({"function_id": "vexit::sync", "payload": {}, "action": CallAction.Void()}) is None
        assert client.call({"function_id": "vexit::async", "payload": {}, "action": CallAction.Void()}) is None
        wait_until(
            lambda: sum("Error invoking function vexit::" in r.getMessage() for r in caplog.records) == 2,
            what="both errors logged",
        )
    messages = " ".join(r.getMessage() for r in caplog.records)
    assert "sync void exited" in messages and "async void interrupted" in messages
    assert client._thread.is_alive()
