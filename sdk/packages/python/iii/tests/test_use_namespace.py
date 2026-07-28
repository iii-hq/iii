"""Namespace-view (`use_namespace`) and namespace-carrying-ref behavior.

Mirrors the Node `use-namespace.type-check.ts` guard, but as runtime unit tests
that need no live engine. They pin:
  - the `_resolve_function_target` normalizer (string vs ref),
  - `register_function` returning a ref that carries `function_id` and `namespace`,
  - namespace resolution for `register_trigger` / `trigger` (explicit > ref > own),
  - `use_namespace` caching, self-return for the own namespace, and view teardown.

The clients connect to an unreachable address with retries disabled, so no engine
is required: registration is stored locally and the trigger wire message is
captured via a patched `_send`.
"""

import asyncio
from collections.abc import Iterator

import pytest
from iii_helpers.observability import ReconnectionConfig

from iii import InitOptions, TriggerAction
from iii.iii import III, _resolve_function_target
from iii.iii_constants import FunctionRef

UNREACHABLE = "ws://127.0.0.1:1"


def _make(namespace: str | None = None) -> III:
    return III(
        UNREACHABLE,
        InitOptions(
            worker_name="ns-test",
            namespace=namespace,
            otel={"enabled": False},
            reconnection_config=ReconnectionConfig(max_retries=0),
        ),
    )


@pytest.fixture
def worker() -> Iterator[III]:
    w = _make(namespace="ns")
    yield w
    w.shutdown()


def _noop_handler(data: object) -> object:
    return data


# --- _resolve_function_target ------------------------------------------------


def test_resolve_target_bare_string() -> None:
    assert _resolve_function_target("greet") == ("greet", None)


def test_resolve_target_ref_carries_namespace() -> None:
    ref = FunctionRef(id="run", unregister=lambda: None, namespace="agent")
    assert _resolve_function_target(ref) == ("run", "agent")


def test_resolve_target_ref_without_namespace() -> None:
    ref = FunctionRef(id="run", unregister=lambda: None)
    assert _resolve_function_target(ref) == ("run", None)


# --- FunctionRef -------------------------------------------------------------


def test_function_ref_defaults_function_id_to_id() -> None:
    ref = FunctionRef(id="run", unregister=lambda: None)
    assert ref.function_id == "run"
    assert ref.namespace is None


# --- register_function ref ---------------------------------------------------


def test_register_function_ref_carries_namespace(worker: III) -> None:
    ref = worker.register_function("run", _noop_handler)
    assert ref.function_id == "run"
    assert ref.namespace == "ns"


def test_register_function_ref_default_namespace_is_none() -> None:
    w = _make()  # no namespace -> engine default
    try:
        ref = w.register_function("run", _noop_handler)
        assert ref.namespace is None
    finally:
        w.shutdown()


# --- register_trigger resolution (inspect stored wire message) ---------------


def _last_trigger_namespace(w: III) -> str | None:
    return next(reversed(w._triggers.values())).namespace


def test_register_trigger_inherits_worker_namespace(worker: III) -> None:
    worker.register_trigger({"type": "cron", "function_id": "greet", "config": {}})
    assert _last_trigger_namespace(worker) == "ns"


def test_register_trigger_uses_ref_namespace_over_own(worker: III) -> None:
    ref = FunctionRef(id="run", unregister=lambda: None, namespace="agent")
    worker.register_trigger({"type": "cron", "function_id": ref, "config": {}})
    assert _last_trigger_namespace(worker) == "agent"


def test_register_trigger_explicit_namespace_wins(worker: III) -> None:
    ref = FunctionRef(id="run", unregister=lambda: None, namespace="agent")
    worker.register_trigger(
        {"type": "cron", "function_id": ref, "config": {}, "namespace": "explicit"}
    )
    assert _last_trigger_namespace(worker) == "explicit"


# --- trigger resolution (capture void wire message) --------------------------


def _capture_void_namespace(w: III, request: dict[str, object]) -> str | None:
    captured: dict[str, object] = {}

    async def fake_send(msg: object) -> None:
        captured.update(w._to_dict(msg))

    w._send = fake_send  # type: ignore[method-assign]
    w.trigger({**request, "action": TriggerAction.Void()})
    return captured.get("namespace")  # type: ignore[return-value]


def test_trigger_inherits_worker_namespace(worker: III) -> None:
    assert _capture_void_namespace(worker, {"function_id": "greet", "payload": {}}) == "ns"


def test_trigger_uses_ref_namespace_over_own(worker: III) -> None:
    ref = FunctionRef(id="run", unregister=lambda: None, namespace="agent")
    assert _capture_void_namespace(worker, {"function_id": ref, "payload": {}}) == "agent"


def test_trigger_explicit_namespace_wins(worker: III) -> None:
    ref = FunctionRef(id="run", unregister=lambda: None, namespace="agent")
    result = _capture_void_namespace(
        worker, {"function_id": ref, "payload": {}, "namespace": "explicit"}
    )
    assert result == "explicit"


# --- use_namespace -----------------------------------------------------------


def test_use_namespace_returns_view_bound_to_namespace() -> None:
    w = _make()  # own namespace normalizes to "default"
    try:
        view = w.use_namespace("agent")
        assert view is not w
        assert view._options.namespace == "agent"
        # A function registered on the view carries the view's namespace.
        assert view.register_function("run", _noop_handler).namespace == "agent"
    finally:
        w.shutdown()


def test_use_namespace_is_cached() -> None:
    w = _make()
    try:
        assert w.use_namespace("agent") is w.use_namespace("agent")
    finally:
        w.shutdown()


def test_use_namespace_own_namespace_returns_self() -> None:
    w = _make(namespace="ns")
    try:
        assert w.use_namespace("ns") is w
    finally:
        w.shutdown()


def test_use_namespace_default_when_no_namespace_returns_self() -> None:
    w = _make()  # no namespace -> lives in "default"
    try:
        assert w.use_namespace("default") is w
    finally:
        w.shutdown()


def test_shutdown_tears_down_views() -> None:
    w = _make()
    view = w.use_namespace("agent")
    # The view runs its own loop thread (started in __init__, deterministic).
    assert view._thread.is_alive()
    w.shutdown()
    # Tearing down the parent tears down the view: its thread is joined/stopped.
    assert not view._thread.is_alive()
    assert view._running is False


def test_trigger_async_resolution(worker: III) -> None:
    # Exercise the async path directly to pin the same resolution as sync trigger.
    ref = FunctionRef(id="run", unregister=lambda: None, namespace="agent")
    captured: dict[str, object] = {}

    async def fake_send(msg: object) -> None:
        captured.update(worker._to_dict(msg))

    worker._send = fake_send  # type: ignore[method-assign]

    async def drive() -> None:
        await worker.trigger_async(
            {"function_id": ref, "payload": {}, "action": TriggerAction.Void()}
        )

    asyncio.run_coroutine_threadsafe(drive(), worker._loop).result(timeout=5)
    assert captured.get("namespace") == "agent"
