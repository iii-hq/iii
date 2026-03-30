"""Integration tests for Worker RBAC module."""

import os
import time

import pytest

from iii import (
    AuthInput,
    AuthResult,
    InitOptions,
    MiddlewareFunctionInput,
    OnFunctionRegistrationInput,
    OnTriggerRegistrationInput,
    OnTriggerTypeRegistrationInput,
    TriggerConfig,
    TriggerHandler,
    register_worker,
)

ENGINE_WS_URL = os.environ.get("III_URL", "ws://localhost:49199")
EW_URL = os.environ.get("III_RBAC_WORKER_URL", "ws://localhost:49135")

auth_calls: list[AuthInput] = []
trigger_type_reg_calls: list[OnTriggerTypeRegistrationInput] = []
trigger_reg_calls: list[OnTriggerRegistrationInput] = []


@pytest.fixture(scope="module")
def iii_server():
    """Server-side III client that registers auth, middleware, and echo functions."""
    client = register_worker(ENGINE_WS_URL)

    def auth_handler(data: dict) -> dict:
        auth_input = AuthInput(**data)
        auth_calls.append(auth_input)
        token = auth_input.headers.get("x-test-token")

        if not token:
            return AuthResult(
                allowed_functions=[],
                forbidden_functions=[],
                allow_trigger_type_registration=False,
                context={"role": "anonymous", "user_id": "anonymous"},
            ).model_dump()

        if token == "valid-token":
            return AuthResult(
                allowed_functions=["test::ew::valid-token-echo"],
                forbidden_functions=[],
                allow_trigger_type_registration=True,
                context={"role": "admin", "user_id": "user-1"},
            ).model_dump()

        if token == "restricted-token":
            return AuthResult(
                allowed_functions=[],
                forbidden_functions=["test::ew::echo"],
                allow_trigger_type_registration=False,
                context={"role": "restricted", "user_id": "user-2"},
            ).model_dump()

        raise Exception("invalid token")

    def middlware_handler(data: dict) -> dict:
        mid = MiddlewareFunctionInput(**data)
        enriched = {**mid.payload, "_intercepted": True, "_caller": mid.context.get("user_id")}
        return client.trigger({"function_id": mid.function_id, "payload": enriched})

    def echo_handler(data):
        return {"echoed": data}

    def valid_token_echo_handler(data):
        return {"echoed": data, "valid_token": True}

    def meta_public_handler(data):
        return {"meta_echoed": data}

    def private_handler(_data):
        return {"private": True}

    def on_function_reg_handler(data: dict) -> bool:
        reg_input = OnFunctionRegistrationInput(**data)
        return not reg_input.function_id.startswith("denied::")

    def on_trigger_type_reg_handler(data: dict) -> bool:
        reg_input = OnTriggerTypeRegistrationInput(**data)
        trigger_type_reg_calls.append(reg_input)
        return not reg_input.trigger_type_id.startswith("denied-tt::")

    def on_trigger_reg_handler(data: dict) -> bool:
        reg_input = OnTriggerRegistrationInput(**data)
        trigger_reg_calls.append(reg_input)
        return not reg_input.function_id.startswith("denied-trig::")

    client.register_function({"id": "test::rbac-worker::auth"}, auth_handler)
    client.register_function({"id": "test::rbac-worker::middleware"}, middlware_handler)
    client.register_function({"id": "test::rbac-worker::on-function-reg"}, on_function_reg_handler)
    client.register_function({"id": "test::rbac-worker::on-trigger-type-reg"}, on_trigger_type_reg_handler)
    client.register_function({"id": "test::rbac-worker::on-trigger-reg"}, on_trigger_reg_handler)
    class NoopTriggerHandler(TriggerHandler):
        async def register_trigger(self, config: TriggerConfig) -> None:
            pass

        async def unregister_trigger(self, config: TriggerConfig) -> None:
            pass

    client.register_trigger_type(
        {"id": "test-rbac-trigger", "description": "Trigger type for RBAC tests"},
        NoopTriggerHandler(),
    )
    client.register_function({"id": "test::ew::public::echo"}, echo_handler)
    client.register_function({"id": "test::ew::valid-token-echo"}, valid_token_echo_handler)
    client.register_function(
        {"id": "test::ew::meta-public", "metadata": {"ew_public": True}},
        meta_public_handler,
    )
    client.register_function({"id": "test::ew::private"}, private_handler)

    time.sleep(1.0)
    yield client
    client.shutdown()


@pytest.fixture(autouse=True)
def _reset_calls():
    auth_calls.clear()
    trigger_type_reg_calls.clear()
    trigger_reg_calls.clear()


class TestRbacWorkers:
    """RBAC Workers"""

    def test_should_return_auth_result_for_valid_token(self, iii_server):
        iii_client = register_worker(
            EW_URL,
            InitOptions(otel={"enabled": False}, headers={"x-test-token": "valid-token"}),
        )

        try:
            result = iii_client.trigger({
                "function_id": "test::ew::valid-token-echo",
                "payload": {"msg": "hello"},
            })

            assert result["valid_token"] is True
            assert result["echoed"]["msg"] == "hello"
            assert result["echoed"]["_caller"] == "user-1"

            assert len(auth_calls) == 1
            assert auth_calls[0].headers["x-test-token"] == "valid-token"
        finally:
            iii_client.shutdown()

    def test_should_return_error_for_private_function(self, iii_server):
        iii_client = register_worker(
            EW_URL,
            InitOptions(otel={"enabled": False}, headers={"x-test-token": "valid-token"}),
        )

        try:
            with pytest.raises(Exception):
                iii_client.trigger({
                    "function_id": "test::ew::private",
                    "payload": {"msg": "hello"},
                })
        finally:
            iii_client.shutdown()

    def test_should_return_forbidden_functions_for_restricted_token(self, iii_server):
        iii_client = register_worker(
            EW_URL,
            InitOptions(otel={"enabled": False}, headers={"x-test-token": "restricted-token"}),
        )

        try:
            with pytest.raises(Exception):
                iii_client.trigger({
                    "function_id": "test::ew::echo",
                    "payload": {"msg": "hello"},
                })
        finally:
            iii_client.shutdown()

    def test_should_deny_trigger_type_registration_via_hook(self, iii_server):
        iii_client = register_worker(
            EW_URL,
            InitOptions(otel={"enabled": False}, headers={"x-test-token": "valid-token"}),
        )

        try:
            class DeniedHandler(TriggerHandler):
                async def register_trigger(self, config: TriggerConfig) -> None:
                    pass

                async def unregister_trigger(self, config: TriggerConfig) -> None:
                    pass

            iii_client.register_trigger_type(
                {"id": "denied-tt::test", "description": "Should be denied"},
                DeniedHandler(),
            )

            time.sleep(1.0)

            assert len(trigger_type_reg_calls) == 1
            assert trigger_type_reg_calls[0].trigger_type_id == "denied-tt::test"
            assert trigger_type_reg_calls[0].description == "Should be denied"
            assert trigger_type_reg_calls[0].context["user_id"] == "user-1"
        finally:
            iii_client.shutdown()

    def test_should_deny_trigger_registration_via_hook(self, iii_server):
        iii_client = register_worker(
            EW_URL,
            InitOptions(otel={"enabled": False}, headers={"x-test-token": "valid-token"}),
        )

        try:
            iii_client.register_trigger({
                "type": "test-rbac-trigger",
                "function_id": "denied-trig::my-fn",
                "config": {"key": "value"},
            })

            time.sleep(1.0)

            assert len(trigger_reg_calls) == 1
            assert trigger_reg_calls[0].trigger_type == "test-rbac-trigger"
            assert trigger_reg_calls[0].function_id == "denied-trig::my-fn"
            assert trigger_reg_calls[0].context["user_id"] == "user-1"
        finally:
            iii_client.shutdown()

    def test_should_deny_function_registration_via_hook(self, iii_server):
        iii_client = register_worker(
            EW_URL,
            InitOptions(otel={"enabled": False}, headers={"x-test-token": "valid-token"}),
        )

        try:
            iii_client.register_function(
                {"id": "denied::blocked-fn"},
                lambda _data: {"should": "not reach"},
            )

            time.sleep(1.0)

            with pytest.raises(Exception):
                iii_client.trigger({
                    "function_id": "denied::blocked-fn",
                    "payload": {},
                })
        finally:
            iii_client.shutdown()
