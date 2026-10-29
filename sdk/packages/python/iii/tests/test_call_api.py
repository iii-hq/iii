"""The calling API: ``call`` / ``call_async``, with ``trigger`` /
``trigger_async`` as their 0.25 names, and the public surface without the
trigger APIs."""

from __future__ import annotations

import asyncio
import importlib
import warnings
from typing import Any

import pytest
from observe_fake_engine import FakeEngine, function_registered, wait_until

import iii
from iii.iii import III


def test_trigger_and_trigger_async_call_without_a_warning(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    client.register_function("math::double", lambda n: n * 2)
    engine.deliver(function_registered("math::double"))
    wait_until(lambda: client._functions["math::double"].observe.local_dispatch, what="local dispatch")
    with warnings.catch_warnings():
        warnings.simplefilter("error")
        assert client.call({"function_id": "math::double", "payload": 2}) == 4
        assert client.trigger({"function_id": "math::double", "payload": 3}) == 6

        async def from_a_loop() -> tuple[Any, Any]:
            return (
                await client.call_async({"function_id": "math::double", "payload": 4}),
                await client.trigger_async({"function_id": "math::double", "payload": 5}),
            )

        assert asyncio.run(from_a_loop()) == (8, 10)
    assert "deprecated" in (III.trigger.__doc__ or "") and "deprecated" in (III.trigger_async.__doc__ or "")


def test_the_trigger_apis_are_gone() -> None:
    for name in ("register_trigger", "register_trigger_type", "unregister_trigger_type"):
        assert not hasattr(III, name)
    for name in ("ObserveHooks", "Trigger", "TriggerConfig", "TriggerHandler", "TriggerTypeRef"):
        assert not hasattr(iii, name)
    for module in ("iii.trigger", "iii.triggers"):
        with pytest.raises(ModuleNotFoundError):
            importlib.import_module(module)
    assert not hasattr(iii.Observer, "emit") and not hasattr(iii.Observer, "legacy")


def test_the_call_and_observe_api_is_at_the_root() -> None:
    from iii_helpers.queue import EnqueueResult

    for name in (
        "Call",
        "CallAction",
        "CallActionEnqueue",
        "CallActionVoid",
        "CallRequest",
        "Observer",
        "ObserveRejection",
        "AddedHook",
        "RemovedHook",
        "Key",
        "MatchHook",
        "MatchKeys",
        "MatchPredicate",
        "DeliverHook",
        "TriggerAction",
        "TriggerActionVoid",
        "EnqueueResult",
    ):
        assert hasattr(iii, name) and name in iii.__all__, name
    assert iii.EnqueueResult is EnqueueResult
