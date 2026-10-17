"""Deprecated trigger APIs (CONTRACT §5): kept working, with a
``DeprecationWarning`` at the caller and one log warning per process per API."""

from __future__ import annotations

import logging
import warnings
from typing import Any

import pytest
from observe_fake_engine import FakeEngine

import iii.iii as iii_module
from iii.trigger import TriggerConfig, TriggerHandler

GUIDE = "https://iii.dev/docs/upgrading/from-0-24-x"
TRIGGER_MESSAGE = (
    "register_trigger is deprecated and will be removed in a future release. Use iii.observe(source, target) "
    f"instead. Migration guide: {GUIDE}"
)
TRIGGER_TYPE_MESSAGE = (
    "register_trigger_type is deprecated and will be removed in a future release. Register a function with an "
    "observe descriptor (register_function(id, handler, observe=ObserveHooks(...))) instead. Migration guide: "
    f"{GUIDE}"
)


class Handler(TriggerHandler[Any]):
    def __init__(self) -> None:
        self.registered: list[TriggerConfig[Any]] = []

    async def register_trigger(self, config: TriggerConfig[Any]) -> None:
        self.registered.append(config)

    async def unregister_trigger(self, config: TriggerConfig[Any]) -> None:
        pass


def test_warn_once_per_process_per_api_with_the_migration_notice_and_keep_working(
    engine: FakeEngine, connect: Any, monkeypatch: pytest.MonkeyPatch, caplog: pytest.LogCaptureFixture
) -> None:
    monkeypatch.setattr(iii_module, "_deprecations_warned", set())
    client = connect(engine)
    with caplog.at_level(logging.WARNING, logger="iii.iii"), warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        ref = client.register_trigger_type({"id": "webhook", "description": "Webhooks"}, Handler())
        client.register_trigger_type({"id": "webhook-2", "description": "More webhooks"}, Handler())
        trigger = client.register_trigger({"type": "webhook", "function_id": "a::b", "config": {"path": "/x"}})
        client.register_trigger({"type": "webhook", "function_id": "a::c", "config": {}})
        ref.register_trigger("a::d", {"path": "/y"})  # through the helper object
        ref.register_function("a::e", lambda d: d)
        client.unregister_trigger_type({"id": "webhook-2", "description": "More webhooks"})
    logged = [r.getMessage() for r in caplog.records if "deprecated" in r.getMessage()]
    assert logged == [TRIGGER_TYPE_MESSAGE, TRIGGER_MESSAGE]  # once each, never for unregister
    messages = [str(w.message) for w in caught if issubclass(w.category, DeprecationWarning)]
    assert messages.count(TRIGGER_TYPE_MESSAGE) == 2
    assert messages.count(TRIGGER_MESSAGE) == 3
    assert any("unregister_trigger_type is deprecated" in m for m in messages)
    assert any("TriggerTypeRef.register_function is deprecated" in m for m in messages)
    # the language marker points at the caller, through the helper object too
    assert all(w.filename == __file__ for w in caught if issubclass(w.category, DeprecationWarning))

    # still working
    frames = engine.wait_for(lambda: len(engine.frames_of("registertrigger")) == 3, what="the triggers")
    assert frames
    assert [f["id"] for f in engine.frames_of("registertriggertype")] == ["webhook", "webhook-2"]
    trigger.unregister()
    engine.wait_for(lambda: engine.frames_of("unregistertrigger"), what="unregistertrigger")
    with caplog.at_level(logging.WARNING, logger="iii.iii"), pytest.warns(DeprecationWarning):
        client.register_trigger({"type": "webhook", "function_id": "a::f", "config": {}})
    assert [r for r in caplog.records if r.getMessage() == TRIGGER_MESSAGE].__len__() == 1


def test_deprecated_apis_carry_the_docstring_marker() -> None:
    from iii.iii import III
    from iii.triggers import TriggerHandler as Base
    from iii.triggers import TriggerTypeRef

    for fn in (
        III.register_trigger,
        III.register_trigger_type,
        III.unregister_trigger_type,
        TriggerTypeRef.register_trigger,
        TriggerTypeRef.register_function,
        Base,
        TriggerTypeRef,
    ):
        assert ".. deprecated:: 0.25.0" in (fn.__doc__ or ""), fn
