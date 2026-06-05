"""Integration tests for custom trigger type lifecycle across two workers."""

import os
import time
from typing import Any

import pytest

from iii import TriggerAction, TriggerConfig, TriggerHandler, register_worker
from iii.iii import III

ENGINE_WS_URL = os.environ.get("III_URL", "ws://localhost:49199")

TRIGGER_TYPE_ID = "test.tt-lifecycle.python"
CONSUMER_FN = "test.tt-lifecycle.python.consumer"
FIRE_FN = "test.tt-lifecycle.python.fire"
TRIGGER_CONFIG = {"tag": "test"}


class LifecycleTriggerHandler(TriggerHandler[Any]):
    def __init__(self) -> None:
        self.bindings: dict[str, TriggerConfig[Any]] = {}
        self.register_calls: list[TriggerConfig[Any]] = []
        self.unregister_calls: list[TriggerConfig[Any]] = []

    async def register_trigger(self, config: TriggerConfig[Any]) -> None:
        self.bindings[config.id] = config
        self.register_calls.append(config)

    async def unregister_trigger(self, config: TriggerConfig[Any]) -> None:
        stored = self.bindings.pop(config.id, None)
        self.unregister_calls.append(stored if stored is not None else config)


def _wait() -> None:
    time.sleep(0.4)


def _create_provider(handler: LifecycleTriggerHandler) -> III:
    client = register_worker(ENGINE_WS_URL)
    client._wait_until_connected()
    time.sleep(0.3)

    client.register_trigger_type(
        {"id": TRIGGER_TYPE_ID, "description": "Python SDK lifecycle test trigger type"},
        handler,
    )

    def fire_handler(payload: dict[str, Any]) -> dict[str, int]:
        for binding in list(handler.bindings.values()):
            client.trigger(
                {
                    "function_id": binding.function_id,
                    "payload": payload,
                    "action": TriggerAction.Void(),
                }
            )
        return {"fired": len(handler.bindings)}

    client.register_function(FIRE_FN, fire_handler)
    return client


def _create_consumer(handler_calls: list[Any]) -> III:
    client = register_worker(ENGINE_WS_URL)
    client._wait_until_connected()
    time.sleep(0.3)

    def consumer_handler(payload: dict[str, Any]) -> dict[str, Any]:
        handler_calls.append(payload)
        return {"ok": True, "payload": payload}

    client.register_function(CONSUMER_FN, consumer_handler)
    client.register_trigger(
        {
            "type": TRIGGER_TYPE_ID,
            "function_id": CONSUMER_FN,
            "config": TRIGGER_CONFIG,
        }
    )
    _wait()
    return client


@pytest.fixture
def trigger_handler() -> LifecycleTriggerHandler:
    return LifecycleTriggerHandler()


def test_fire_invokes_bound_function(trigger_handler: LifecycleTriggerHandler) -> None:
    provider = _create_provider(trigger_handler)
    handler_calls: list[Any] = []
    consumer = _create_consumer(handler_calls)

    try:
        assert len(trigger_handler.register_calls) == 1
        assert trigger_handler.register_calls[0].function_id == CONSUMER_FN

        provider.trigger({"function_id": FIRE_FN, "payload": {"n": 1}})
        _wait()

        assert len(handler_calls) == 1
        assert handler_calls[0]["n"] == 1
    finally:
        consumer.shutdown()
        provider.shutdown()


def test_provider_reconnect_rebinds_trigger(trigger_handler: LifecycleTriggerHandler) -> None:
    provider = _create_provider(trigger_handler)
    handler_calls: list[Any] = []
    consumer = _create_consumer(handler_calls)

    try:
        bound_trigger_id = trigger_handler.register_calls[0].id
        trigger_handler.register_calls.clear()

        provider.shutdown()
        _wait()

        provider = _create_provider(trigger_handler)
        _wait()

        assert len(trigger_handler.register_calls) == 1
        assert trigger_handler.register_calls[0].id == bound_trigger_id
        assert trigger_handler.register_calls[0].function_id == CONSUMER_FN

        handler_calls.clear()
        provider.trigger({"function_id": FIRE_FN, "payload": {"n": 2}})
        _wait()

        assert len(handler_calls) == 1
        assert handler_calls[0]["n"] == 2
    finally:
        consumer.shutdown()
        provider.shutdown()


def test_consumer_disconnect_invokes_unregister_trigger(
    trigger_handler: LifecycleTriggerHandler,
) -> None:
    provider = _create_provider(trigger_handler)
    handler_calls: list[Any] = []
    consumer = _create_consumer(handler_calls)

    try:
        trigger_handler.unregister_calls.clear()

        consumer.shutdown()
        _wait()

        assert len(trigger_handler.unregister_calls) == 1
        unreg = trigger_handler.unregister_calls[0]
        assert unreg.function_id == CONSUMER_FN
        assert unreg.config == TRIGGER_CONFIG
    finally:
        provider.shutdown()
