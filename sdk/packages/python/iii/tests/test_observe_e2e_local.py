"""Observable functions against the real engine: local dispatch, the reporter,
the observer hooks, matching by keys, deliver hooks that call through an
observer, event sources and private functions."""

from __future__ import annotations

import time
import uuid
from typing import Any

import pytest
from observe_e2e_utils import (
    Activation,
    FrameCounter,
    collector,
    drop_connection,
    exactly,
    flush_client,
    local_dispatch_ready,
    stable,
    wait_for,
)

from iii import Call, InvocationError, Observer, ObserveRejection
from iii.iii import III

ALL = ["input", "output", "error", "caller", "metadata"]

#: Local calls in the zero-wire test (sdk.md §7 asks for 1M; that would take
#: minutes here, so the count is sized to run well under 10 s against a live
#: engine with tracing on and with coverage tracing on, as CI runs it). The
#: positive control below shows the counter does see frames.
ZERO_WIRE_CALLS = 20_000


def test_equivalence_dispatch_auto_and_remote_give_identical_envelopes(worker: Any, fid: Any) -> None:
    owner = worker("eq-owner")
    watcher = worker("eq-watcher")

    def price(data: dict[str, Any]) -> dict[str, Any]:
        if data.get("fail"):
            raise RuntimeError("out of stock")
        return {"total": data["qty"] * 10, "sku": data["sku"]}

    def checkout(data: dict[str, Any]) -> Any:
        return owner.call(
            {
                "function_id": fid("py-eq::price"),
                "payload": {"sku": "A-1", "qty": 3, "fail": data.get("fail")},
                "metadata": {"channel": "web"},
                "dispatch": data["dispatch"],
            }
        )

    owner.register_function(fid("py-eq::price"), price)
    owner.register_function(fid("py-eq::checkout"), checkout)
    got = collector(watcher, fid("py-eq::on-price"))
    handle = watcher.observe(
        {
            "function_id": fid("py-eq::price"),
            "on": "complete",
            "capture": ALL,
            "filters": {"input.sku": {"$prefix": "A-"}},
        },
        fid("py-eq::on-price"),
        {"metadata": {"observer": "eq"}},
    )
    handle.ready(timeout=10)
    local_dispatch_ready(owner, fid("py-eq::price"))
    local_dispatch_ready(owner, fid("py-eq::checkout"))
    frames = FrameCounter(owner)

    def run(dispatch: str, fail: bool) -> Activation:
        before = len(got)
        try:
            owner.call(
                {
                    "function_id": fid("py-eq::checkout"),
                    "payload": {"dispatch": dispatch, "fail": fail},
                    "dispatch": dispatch,
                }
            )
        except InvocationError:
            pass
        wait_for(lambda: len(got) > before, f"the {dispatch} activation")
        return got[-1]

    remote_ok = run("remote", False)
    remote_frames = frames.types.count("invokefunction")
    local_ok = run("auto", False)
    assert frames.types.count("invokefunction") == remote_frames  # no invocation frame
    remote_err = run("remote", True)
    local_err = run("auto", True)

    assert stable(local_ok.data) == stable(remote_ok.data)
    assert stable(local_err.data) == stable(remote_err.data)
    assert local_ok.metadata == remote_ok.metadata == {"observer": "eq"}
    assert local_ok.inv is not None and remote_ok.inv is not None
    assert local_ok.inv.observe_depth == remote_ok.inv.observe_depth == 1
    assert remote_ok.data["input"] == {"sku": "A-1", "qty": 3, "fail": False}
    assert remote_ok.data["output"] == {"total": 30, "sku": "A-1"}
    assert remote_ok.data["metadata"] == {"channel": "web"}
    assert remote_ok.data["caller"]["function_id"] == fid("py-eq::checkout")
    assert remote_err.data["error"]["message"] == "out of stock"


@pytest.mark.parametrize("dispatch", ["auto", "remote"])
def test_capture_empty_moves_no_payloads_and_deliver_output_sends_the_handlers_return_value(
    worker: Any, fid: Any, dispatch: str
) -> None:
    owner = worker("cap-owner")
    watcher = worker("cap-watcher")
    owner.register_function(fid("py-cap::price"), lambda data: {"total": data["qty"] * 10})
    envelopes = collector(watcher, fid("py-cap::on-envelope"))
    outputs = collector(watcher, fid("py-cap::on-output"))
    bare = watcher.observe({"function_id": fid("py-cap::price"), "capture": []}, fid("py-cap::on-envelope"))
    output = watcher.observe({"function_id": fid("py-cap::price"), "deliver": "output"}, fid("py-cap::on-output"))
    bare.ready(timeout=10)
    output.ready(timeout=10)
    local_dispatch_ready(owner, fid("py-cap::price"))
    frames = FrameCounter(owner)

    request = {"function_id": fid("py-cap::price"), "payload": {"qty": 3}, "metadata": {"channel": "web"}}
    assert owner.call({**request, "dispatch": dispatch}) == {"total": 30}
    exactly(envelopes, 1, "the envelope")
    exactly(outputs, 1, "the output")

    # A7: deliver 'output' hands the target the handler's return value
    assert outputs[0].data == {"total": 30}
    # A6: capture [] keeps ids, status and timing, and moves no payload
    envelope = envelopes[0].data
    assert not {"input", "output", "error", "caller", "metadata"} & envelope.keys()
    assert (envelope["observer_id"], envelope["function_id"], envelope["status"]) == (
        bare.id,
        fid("py-cap::price"),
        "success",
    )
    assert all(isinstance(envelope[k], str) for k in ("activation_id", "invocation_id"))
    assert isinstance(envelope["started_at_ms"], int) and isinstance(envelope["duration_ms"], (int, float))
    # the host delivered both: a report for the local call, the match on its answer for the routed one
    observed = [sorted(f["observed"]) for f in frames.frames if f.get("type") == "invocationresult" and "observed" in f]
    if dispatch == "auto":
        assert "invocationsobserved" in frames.types and observed == []
    else:
        assert "invocationsobserved" not in frames.types and observed == [sorted([bare.id, output.id])]


def test_zero_wire_unobserved_local_calls_send_zero_frames(worker: Any, fid: Any) -> None:
    owner = worker("zero-wire")
    owner.register_function(fid("py-zw::inc"), lambda n: n + 1)
    owner.register_function(fid("py-zw::control"), lambda n: n)
    watcher = worker("zero-wire-watcher")
    collector(watcher, fid("py-zw::target"))
    control = watcher.observe({"function_id": fid("py-zw::control"), "capture": ["input"]}, fid("py-zw::target"))
    control.ready(timeout=10)
    local_dispatch_ready(owner, fid("py-zw::inc"))
    local_dispatch_ready(owner, fid("py-zw::control"))
    wait_for(lambda: owner._functions[fid("py-zw::control")].observe.is_observed(), "the control observer")
    frames = FrameCounter(owner)
    for i in range(ZERO_WIRE_CALLS):
        owner.call({"function_id": fid("py-zw::inc"), "payload": i})
    flush_client(owner)  # anything a call put on the wire is written by now
    assert [t for t in frames.types if t != "localinvocationstats"] == []
    # positive control: the counter does see an observed call's report
    owner.call({"function_id": fid("py-zw::control"), "payload": 1})
    wait_for(lambda: "invocationsobserved" in frames.types, "the control's report")


def test_activation_race_after_ready_every_local_invocation_is_reported(worker: Any, fid: Any) -> None:
    n = 200
    owner = worker("race-owner")
    watcher = worker("race-watcher")
    owner.register_function(fid("py-race::fn"), lambda x: x)
    local_dispatch_ready(owner, fid("py-race::fn"))
    got = collector(watcher, fid("py-race::target"))
    handle = watcher.observe(
        {"function_id": fid("py-race::fn"), "capture": ["input"]}, fid("py-race::target"), {"max_in_flight": n}
    )
    handle.ready(timeout=10)
    frames = FrameCounter(owner)
    for i in range(n):
        owner.call({"function_id": fid("py-race::fn"), "payload": i})
    flush_client(owner)
    sent = list(frames.types)  # before the barrier below, which makes calls of its own
    exactly(got, n, f"{n} activations", timeout=10)
    assert len({g.data["input"] for g in got}) == n
    assert "invokefunction" not in sent
    assert "invocationsobserved" in sent


class Cron:
    """A cron-like source: its handler returns the tick, its hooks keep one index entry per expression, and its
    match keys give each tick to that expression's observers."""

    def __init__(self, client: III) -> None:
        self.client = client
        # One id per test: observers of an earlier test stay attached (suspended)
        # for the engine's grace period, and would be pushed to this worker too.
        self.fn = f"py-cron-{uuid.uuid4().hex[:8]}::on-tick"
        self.index: dict[str, list[Observer]] = {}
        self.added: list[Observer] = []
        self.removed: list[Observer] = []
        #: What happened, in order (hooks returning, the test's own steps).
        self.events: list[str] = []
        self.reject_expression: str | None = None
        self.add_delay = 0.0

        # by id, before registering: the hooks run for every observer
        @client.on_observer_added(self.fn)
        def on_added(obs: Observer) -> None:
            self.added.append(obs)
            if self.add_delay:
                time.sleep(self.add_delay)
            if self.reject_expression is not None and obs.args["expression"] == self.reject_expression:
                raise ObserveRejection("INVALID_CRON_EXPRESSION", f"bad expression {self.reject_expression}")
            self.index.setdefault(obs.args["expression"], []).append(obs)
            self.events.append(f"hook returned {obs.id}")

        @client.on_observer_removed(self.fn)
        def on_removed(obs: Observer) -> None:
            self.removed.append(obs)
            if obs in self.index.get(obs.args["expression"], []):
                self.index[obs.args["expression"]].remove(obs)

        client.on_observer_match(
            self.fn, observer=lambda obs: obs.args["expression"], call=lambda call: call.input["expression"]
        )
        self.ref = client.register_function(
            self.fn,
            lambda tick: tick,
            args_format={
                "type": "object",
                "additionalProperties": False,
                "required": ["expression"],
                "properties": {"expression": {"type": "string"}},
            },
            private=True,
        )


def test_args_are_what_the_observer_sent_and_bad_args_are_invalid_args_before_any_hook(worker: Any, fid: Any) -> None:
    cron = Cron(worker("cron-owner"))
    watcher = worker("args-watcher")
    watcher.register_function(fid("py-args::target"), lambda d: None)
    watcher.register_function(fid("py-args::auto"), lambda d: None)
    ok = watcher.observe({"function_id": cron.fn, "args": {"expression": "*/5"}}, fid("py-args::target"))
    ok.ready(timeout=10)
    assert [o.args for o in cron.added] == [{"expression": "*/5"}]
    missing = watcher.observe({"function_id": cron.fn, "args": {}}, fid("py-args::target"))
    with pytest.raises(ObserveRejection) as raised:
        missing.ready(timeout=10)
    assert raised.value.code == "INVALID_ARGS"
    on_auto = watcher.observe(
        {"function_id": fid("py-args::auto"), "args": {"expression": "*/5"}}, fid("py-args::target")
    )
    with pytest.raises(ObserveRejection) as raised:
        on_auto.ready(timeout=10)
    assert raised.value.code == "INVALID_ARGS"
    assert len(cron.added) == 1


def test_observers_matched_by_keys_only_receive_their_keys_calls_even_right_after_ready(worker: Any, fid: Any) -> None:
    cron = Cron(worker("cron-owner"))
    watcher = worker("routing-watcher")
    five = collector(watcher, fid("py-routing::five"))
    ten = collector(watcher, fid("py-routing::ten"))
    a = watcher.observe({"function_id": cron.fn, "args": {"expression": "*/5"}}, fid("py-routing::five"))
    b = watcher.observe({"function_id": cron.fn, "args": {"expression": "*/10"}}, fid("py-routing::ten"))
    a.ready(timeout=10)
    b.ready(timeout=10)
    local_dispatch_ready(cron.client, cron.fn)
    # right after ready(): the source filed the observer before acking it
    cron.client.call({"function_id": cron.fn, "payload": {"expression": "*/10", "n": 1}})
    cron.client.call({"function_id": cron.fn, "payload": {"expression": "*/5", "n": 2}})
    cron.client.call({"function_id": cron.fn, "payload": {"expression": "* * *", "n": 3}})
    exactly(ten, 1, "the */10 activation")
    exactly(five, 1, "the */5 activation")
    assert ten[0].data["observer_id"] == b.id and ten[0].data["input"]["n"] == 1
    assert ten[0].data["function_id"] == cron.fn
    assert five[0].data["observer_id"] == a.id and five[0].data["input"]["n"] == 2


def test_hooks_once_per_add_before_the_ack_a_raise_rejects_ready_the_engine_keeps_the_filters_removed_on_dispose(
    worker: Any,
    fid: Any,
) -> None:
    cron = Cron(worker("cron-owner"))
    cron.reject_expression = "bad"
    cron.add_delay = 0.3
    watcher = worker("hooks-watcher")
    watcher.register_function(fid("py-hooks::target"), lambda d: None)
    filters = {"input.n": {"$gte": 2}}
    ok = watcher.observe(
        {"function_id": cron.fn, "args": {"expression": "*/5"}, "filters": filters}, fid("py-hooks::target")
    )
    ok.ready(timeout=10)
    cron.events.append(f"ready {ok.id}")
    # active only after the hook returned (it sleeps first, so an early `active` would come before it)
    assert cron.events == [f"hook returned {ok.id}", f"ready {ok.id}"]
    assert len([o for o in cron.added if o.id == ok.id]) == 1
    assert cron.added[0].filters == {"$and": [{"input.n": {"$gte": 2}}]}
    got = watcher.call({"function_id": "engine::observers::get", "payload": {"id": ok.id}})
    assert '"input.n"' in __import__("json").dumps(got)

    bad = watcher.observe({"function_id": cron.fn, "args": {"expression": "bad"}}, fid("py-hooks::target"))
    with pytest.raises(ObserveRejection) as raised:
        bad.ready(timeout=10)
    assert raised.value.code == "INVALID_CRON_EXPRESSION"
    assert bad.state == "error"

    ok.dispose()
    wait_for(lambda: any(o.id == ok.id for o in cron.removed), "on_observer_removed after dispose")
    assert ok.id not in [o.id for o in cron.ref.observers()]


def test_a_call_no_filter_matches_sends_no_frame_and_the_target_sees_the_envelope_and_the_metadata_sidecar(
    worker: Any,
    fid: Any,
) -> None:
    cron = Cron(worker("cron-owner"))
    watcher = worker("filtered-watcher")
    got = collector(watcher, fid("py-filtered::target"))
    handle = watcher.observe(
        {
            "function_id": cron.fn,
            "args": {"expression": "*/5"},
            "filters": {"input.n": {"$gt": 10}},
            "capture": ["input", "caller"],
        },
        fid("py-filtered::target"),
        {"metadata": {"__binding": "digest"}},
    )
    handle.ready(timeout=10)
    local_dispatch_ready(cron.client, cron.fn)
    frames = FrameCounter(cron.client)
    cron.client.call({"function_id": cron.fn, "payload": {"expression": "*/5", "n": 1}})
    flush_client(cron.client)
    assert frames.count == 0
    cron.client.call({"function_id": cron.fn, "payload": {"expression": "*/5", "n": 11}})
    exactly(got, 1, "the activation")
    data = got[0].data
    assert (data["observer_id"], data["status"]) == (handle.id, "success")
    assert data["input"] == {"expression": "*/5", "n": 11}
    assert "cron-owner" in data["caller"]["worker_name"]
    assert got[0].metadata == {"__binding": "digest"}


def test_a_deliver_hook_calls_through_the_observer_its_filters_apply_and_a_disposed_one_is_refused(
    worker: Any,
    fid: Any,
) -> None:
    owner = worker("http-owner")
    on_request_id = f"py-http-{uuid.uuid4().hex[:8]}::on-request"
    routes: dict[str, Observer] = {}
    owner.on_observer_added(on_request_id, lambda obs: routes.__setitem__(obs.args["path"], obs))
    owner.on_observer_removed(on_request_id, lambda obs: routes.pop(obs.args["path"], None))

    @owner.on_observer_deliver(on_request_id)
    def route(call: Call[Any, Any], observers: Any) -> Any:
        obs = routes.get(call.input["path"])
        if obs is None:
            return {"status": 404}
        if not obs.matches(call):
            return {"status": 422}
        return owner.call({"observer": obs, "payload": call.input})

    owner.register_function(
        on_request_id,
        lambda request: request,
        args_format={"type": "object", "required": ["path"], "properties": {"path": {"type": "string"}}},
        private=True,
    )
    app = worker("http-app")
    metadata_seen: list[Any] = []

    def users_get(req: dict[str, Any], metadata: Any = None) -> dict[str, Any]:
        metadata_seen.append(metadata)
        return {"status": 200, "body": {"path": req["path"]}}

    users_get_id = fid("py-http::users-get")
    app.register_function(users_get_id, users_get)
    route_handle = app.observe(
        {"function_id": on_request_id, "args": {"path": "/users"}, "filters": {"input.tenant": {"$eq": "acme"}}},
        users_get_id,
        {"metadata": {"route": "users"}},
    )
    route_handle.ready(timeout=10)
    local_dispatch_ready(owner, on_request_id)

    assert owner.call({"function_id": on_request_id, "payload": {"path": "/users", "tenant": "acme"}}) == {
        "status": 200,
        "body": {"path": "/users"},
    }
    assert metadata_seen == [{"route": "users"}]  # the observer's metadata sidecar
    assert owner.call({"function_id": on_request_id, "payload": {"path": "/users", "tenant": "x"}}) == {"status": 422}
    assert owner.call({"function_id": on_request_id, "payload": {"path": "/nope"}}) == {"status": 404}

    # D1: the function is private, so only the worker that hosts it calls it
    with pytest.raises(InvocationError) as raised:
        app.call({"function_id": on_request_id, "payload": {"path": "/users", "tenant": "acme"}})
    assert raised.value.code == "FORBIDDEN"

    # C3: only the worker that hosts the observed function calls through its observers
    obs = routes["/users"]
    with pytest.raises(InvocationError) as raised:
        app.call({"observer": obs, "payload": {"path": "/users", "tenant": "acme"}})
    assert raised.value.code == "NOT_FUNCTION_OWNER"

    route_handle.dispose()
    wait_for(lambda: "/users" not in routes, "on_observer_removed")
    with pytest.raises(InvocationError) as raised:
        owner.call({"observer": obs, "payload": {"path": "/users", "tenant": "acme"}})
    assert raised.value.code in ("OBSERVER_NOT_FOUND", "OBSERVER_NOT_ACTIVE")


def test_an_event_source_sends_each_event_once_and_its_observers_observe_the_event(worker: Any, fid: Any) -> None:
    source = worker("source")
    watcher = worker("source-watcher")
    source.register_function(fid("py-src::on-thing"), lambda event: event, private=True)
    got = collector(watcher, fid("py-src::target"))
    handle = watcher.observe(fid("py-src::on-thing"), fid("py-src::target"))
    handle.ready(timeout=10)
    local_dispatch_ready(source, fid("py-src::on-thing"))
    frames = FrameCounter(source)
    event = {"kind": "thing", "n": 7}
    assert source.call({"function_id": fid("py-src::on-thing"), "payload": event}) is event  # by reference
    exactly(got, 1, "the activation")
    assert got[0].data["input"] == got[0].data["output"] == event
    assert frames.items[0].get("output_is_input") is True and "output" not in frames.items[0]


def test_reconnect_local_dispatch_keeps_serving_reports_flush_after_and_the_diff_calls_the_right_hooks(
    worker: Any,
    fid: Any,
) -> None:
    owner = worker("rc-owner")
    watcher = worker("rc-watcher")
    owner.register_function(fid("py-rc::fn"), lambda n: n * 2)
    got = collector(watcher, fid("py-rc::target"))
    kept = watcher.observe({"function_id": fid("py-rc::fn"), "capture": ["input"]}, fid("py-rc::target"))
    kept.ready(timeout=10)
    local_dispatch_ready(owner, fid("py-rc::fn"))
    wait_for(lambda: owner._functions[fid("py-rc::fn")].observe.is_observed(), "the observer on the owner")

    drop_connection(owner)
    wait_for(lambda: owner.get_connection_state() != "connected", "the disconnect")
    assert owner.call({"function_id": fid("py-rc::fn"), "payload": 21}) == 42  # still local
    wait_for(lambda: owner._registered_on_connection, "the reconnect", timeout=10)
    exactly(got, 1, "the activation reported after the reconnect", timeout=10)
    assert got[0].data["input"] == 21

    # the diff: an observer removed while the owner was away
    added: list[str] = []
    removed: list[str] = []
    source = owner.register_function(fid("py-rc::source"), lambda d: None)
    source.on_observer_added(lambda o: added.append(o.id))
    source.on_observer_removed(lambda o: removed.append(o.id))
    gone = watcher.observe({"function_id": fid("py-rc::source")}, fid("py-rc::target"))
    stays = watcher.observe({"function_id": fid("py-rc::source")}, fid("py-rc::target"))
    gone.ready(timeout=10)
    stays.ready(timeout=10)
    assert sorted(added) == sorted([gone.id, stays.id])

    # a slow reconnect, so the dispose lands while the owner is away
    owner._reconnection_config = type(owner._reconnection_config)(
        max_retries=20, initial_delay_ms=800, max_delay_ms=800, jitter_factor=0.0
    )
    drop_connection(owner)
    wait_for(lambda: owner.get_connection_state() != "connected", "the disconnect")
    gone.dispose()
    wait_for(lambda: owner._registered_on_connection, "the reconnect", timeout=10)
    wait_for(lambda: gone.id in removed, "on_observer_removed from the diff", timeout=10)
    assert removed == [gone.id]
    assert added.count(stays.id) == 1  # kept: no second add
