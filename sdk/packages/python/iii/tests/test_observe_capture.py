"""D1 / D6 of the observe.capture contract (AMENDMENT 1: the view field is
``fires_on``): the default capture, the ``observe.capture`` feature, the outcomes
an observer fires on read from its view's ``fires_on`` (or else its legacy
``on``), and the legacy ``source.on`` sent to an engine without
``observe.capture``. Against a fake engine."""

from __future__ import annotations

import warnings
from typing import Any

import pytest
from observe_fake_engine import (
    V1_FEATURES,
    FakeEngine,
    function_registered,
    observer_added,
    reported_items,
    settle,
    wait_until,
)

from iii.iii import III
from iii.iii_types import DEFAULT_CAPTURE, SDK_FEATURES, ObserverInfoWire

RC_ENGINE = ["observe.v1", "observe.v2", "local-dispatch.v1"]
CAPTURE_ENGINE = ["observe.v1", "observe.v2", "observe.capture", "local-dispatch.v1"]
#: A distinctive part of the one-time deprecation warning.
DEPRECATION = "does not support observe.capture"


def _deprecations(caught: list[warnings.WarningMessage]) -> list[warnings.WarningMessage]:
    return [w for w in caught if issubclass(w.category, FutureWarning) and DEPRECATION in str(w.message)]


def _confirm(client: III, engine: FakeEngine, function_id: str, observers: list[dict[str, Any]]) -> None:
    engine.deliver(function_registered(function_id, observers))
    state = client._functions[function_id].observe
    wait_until(lambda: state.confirmed, what=f"functionregistered for {function_id}")
    ids = {o["id"] for o in observers}
    wait_until(
        lambda: ids <= {f.get("id") for f in engine.frames_of("observerack") if f.get("function_id") == function_id},
        what="the acks",
    )


# ── D1 ──────────────────────────────────────────────────────────────────────


def test_the_default_capture_is_input_output_caller() -> None:
    assert DEFAULT_CAPTURE == ("input", "output", "caller")
    assert ObserverInfoWire.model_validate({"id": "o", "target": "t"}).capture == ["input", "output", "caller"]
    assert ObserverInfoWire.model_validate({"id": "o", "target": "t", "capture": None}).capture == [
        "input",
        "output",
        "caller",
    ]


# ── D6: the feature ─────────────────────────────────────────────────────────


def test_the_sdk_announces_observe_capture() -> None:
    assert "observe.capture" in SDK_FEATURES


# ── D6: firing from the view ────────────────────────────────────────────────

VIEW_CASES: list[tuple[dict[str, Any], bool, bool]] = [
    # an engine with observe.capture: fires_on is all that counts
    ({"fires_on": ["error"], "capture": ["input", "output"]}, False, True),
    ({"fires_on": ["success"], "capture": ["error"]}, True, False),
    ({"fires_on": ["success", "error"], "capture": []}, True, True),
    ({"fires_on": ["error"], "capture": ["input"], "on": "complete"}, False, True),
    ({"fires_on": ["success"]}, True, False),
    # no fires_on (rc or 0.25 engine): on alone, never the capture
    ({"capture": ["input", "output", "error"]}, True, False),
    ({"capture": ["error"]}, True, False),
    ({"capture": ["error"], "on": None}, True, False),
    ({"capture": ["input"], "on": "success"}, True, False),
    ({"capture": ["input"], "on": "error"}, False, True),
    ({"capture": ["input"], "on": "complete"}, True, True),
    ({"capture": ["error"], "on": "error"}, False, True),
    ({"capture": [], "on": "complete"}, True, True),
    ({}, True, False),
    # fires is not the outcome list (AMENDMENT 1): ignored
    ({"fires": ["error"], "capture": ["input"]}, True, False),
    ({"fires": 3, "fires_on": ["error"]}, False, True),
]


def test_an_observer_fires_on_the_outcomes_its_view_announces(engine_factory: Any, connect: Any) -> None:
    engine = engine_factory(RC_ENGINE)
    client = connect(engine)
    client.register_function("f::fn", lambda d: d)
    views = [
        {"id": f"o{i}", "args": {}, "filters": {}, "target": "t", **extra} for i, (extra, _, _) in enumerate(VIEW_CASES)
    ]
    _confirm(client, engine, "f::fn", views)
    by_id = {obs.id: obs for obs in client.observers("f::fn")}
    for i, (extra, on_success, on_error) in enumerate(VIEW_CASES):
        obs = by_id[f"o{i}"]
        assert obs.matches({"status": "success", "input": {}, "output": {}}) is on_success, f"success: {extra}"
        assert obs.matches({"status": "error", "input": {}}) is on_error, f"error: {extra}"


@pytest.mark.parametrize(
    ("on", "want"),
    [(None, ["success"]), ("success", ["success"]), ("error", ["error"]), ("complete", ["error", "success"])],
)
def test_a_view_without_fires_on_fires_as_its_legacy_on_says(
    engine_factory: Any, connect: Any, on: str | None, want: list[str]
) -> None:
    """An rc engine (observe.v2 without observe.capture) announces views without
    ``fires_on``: the observer fires as its legacy ``on`` says (absent: success
    only), never as its capture implies."""
    engine = engine_factory(RC_ENGINE)
    client = connect(engine)

    def charge(data: dict[str, Any]) -> Any:
        if data.get("fail"):
            raise RuntimeError("declined")
        return {"ok": True}

    client.register_function("p::charge", charge)
    # the capture alone would fire on both outcomes
    view: dict[str, Any] = {"id": "obs", "args": {}, "filters": {}, "target": "t", "capture": ["input", "error"]}
    if on is not None:
        view["on"] = on
    _confirm(client, engine, "p::charge", [view])
    engine.clear()
    client.call({"function_id": "p::charge", "payload": {"amount": 1}})
    with pytest.raises(Exception):
        client.call({"function_id": "p::charge", "payload": {"amount": 2, "fail": True}})
    settle(client, engine)
    assert sorted(item["status"] for item in reported_items(engine)) == want


def test_a_view_without_fires_on_warns_once(engine_factory: Any, connect: Any) -> None:
    engine = engine_factory(CAPTURE_ENGINE)
    client = connect(engine)
    client.register_function("f::fn", lambda d: d)
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        _confirm(
            client,
            engine,
            "f::fn",
            [{"id": "a", "target": "t", "capture": ["input"], "fires_on": ["success"]}],
        )
        settle(client, engine)
        assert _deprecations(caught) == []
        for revision, observer_id in [(1, "b"), (2, "c")]:
            engine.deliver(observer_added("f::fn", revision, {"id": observer_id, "target": "t", "capture": ["input"]}))
        wait_until(
            lambda: {"b", "c"} <= {f.get("id") for f in engine.frames_of("observerack")},
            what="the acks",
        )
        settle(client, engine)
    assert len(_deprecations(caught)) == 1


# ── D6: the `on` sent ───────────────────────────────────────────────────────

LEGACY_ON_CASES: list[tuple[list[str] | None, str | None]] = [
    (None, None),
    ([], None),
    (["input"], None),
    (["input", "output", "caller", "metadata"], None),
    (["error"], "error"),
    (["error", "caller"], "error"),
    (["input", "error"], "complete"),
    (["input", "output", "error"], "complete"),
]


def _sent_on(engine: FakeEngine, connection: int) -> dict[str, Any]:
    return {frame["id"]: frame["source"].get("on") for frame in engine.frames_of("observe", connection)}


def _observe_each_and_replay(client: III, engine: FakeEngine) -> tuple[dict[str, Any], dict[str, Any]]:
    for i, (capture, _) in enumerate(LEGACY_ON_CASES):
        source: dict[str, Any] = {"function_id": "f::a"}
        if capture is not None:
            source["capture"] = capture
        client.observe(source, "t::a", {"id": f"o{i}"})
    engine.wait_for(lambda: len(engine.frames_of("observe", 0)) == len(LEGACY_ON_CASES), what="the observes")
    engine.drop()
    wait_until(lambda: len(engine.connections) == 2, what="the reconnect")
    engine.wait_for(lambda: len(engine.frames_of("observe", 1)) == len(LEGACY_ON_CASES), what="the replay")
    return _sent_on(engine, 0), _sent_on(engine, 1)


@pytest.mark.parametrize("features", [V1_FEATURES, RC_ENGINE], ids=["0.25", "rc"])
def test_an_engine_without_observe_capture_gets_the_on_capture_implies(
    engine_factory: Any, connect: Any, features: list[str]
) -> None:
    engine = engine_factory(features)
    client = connect(engine)
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        first, replay = _observe_each_and_replay(client, engine)
    want = {f"o{i}": on for i, (_, on) in enumerate(LEGACY_ON_CASES)}
    assert first == want
    assert replay == want
    assert len(_deprecations(caught)) == 1


def test_an_engine_with_observe_capture_gets_no_on_and_no_warning(engine_factory: Any, connect: Any) -> None:
    engine = engine_factory(CAPTURE_ENGINE)
    client = connect(engine)
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        first, replay = _observe_each_and_replay(client, engine)
    want = {f"o{i}": None for i in range(len(LEGACY_ON_CASES))}
    assert first == want
    assert replay == want
    assert _deprecations(caught) == []
    assert all("on" not in frame["source"] for frame in engine.frames_of("observe"))
