"""Integration tests for Worker RBAC module."""

import os
import time

import pytest

from iii import (
    AuthInput,
    AuthResult,
    FunctionInfo,
    IIIForbiddenError,
    IIIInvocationError,
    InitOptions,
    MiddlewareFunctionInput,
    OnFunctionRegistrationInput,
    OnFunctionRegistrationResult,
    OnTriggerRegistrationInput,
    OnTriggerRegistrationResult,
    OnTriggerTypeRegistrationInput,
    OnTriggerTypeRegistrationResult,
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

        if token == "prefix-token":
            return AuthResult(
                allowed_functions=[],
                forbidden_functions=[],
                allow_trigger_type_registration=True,
                context={"role": "prefixed", "user_id": "user-prefix"},
                function_registration_prefix="test-prefix",
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

    def on_function_reg_handler(data: dict) -> dict:
        reg_input = OnFunctionRegistrationInput(**data)
        if reg_input.function_id.startswith("denied::"):
            raise Exception("denied function registration")
        return OnFunctionRegistrationResult(
            function_id=reg_input.function_id,
        ).model_dump()

    def on_trigger_type_reg_handler(data: dict) -> dict:
        reg_input = OnTriggerTypeRegistrationInput(**data)
        trigger_type_reg_calls.append(reg_input)
        if reg_input.trigger_type_id.startswith("denied-tt::"):
            raise Exception("denied trigger type registration")
        return OnTriggerTypeRegistrationResult().model_dump()

    def on_trigger_reg_handler(data: dict) -> dict:
        reg_input = OnTriggerRegistrationInput(**data)
        trigger_reg_calls.append(reg_input)
        if reg_input.function_id.startswith("denied-trig::"):
            raise Exception("denied trigger registration")
        return OnTriggerRegistrationResult().model_dump()

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

    def test_function_discovery_only_returns_allowed_for_valid_token(self, iii_server):
        iii_client = register_worker(
            EW_URL,
            InitOptions(otel={"enabled": False}, headers={"x-test-token": "valid-token"}),
        )

        try:
            time.sleep(1.0)

            result = iii_client.trigger(
                {"function_id": "engine::functions::list", "payload": {}}
            )
            functions = [FunctionInfo(**f) for f in result.get("functions", [])]
            function_ids = [f.function_id for f in functions]

            assert "test::ew::valid-token-echo" in function_ids
            assert "test::ew::public::echo" in function_ids
            assert "test::ew::meta-public" in function_ids

            assert "test::ew::private" not in function_ids
            assert "test::rbac-worker::auth" not in function_ids
        finally:
            iii_client.shutdown()

    def test_function_discovery_only_returns_exposed_for_restricted_token(self, iii_server):
        iii_client = register_worker(
            EW_URL,
            InitOptions(otel={"enabled": False}, headers={"x-test-token": "restricted-token"}),
        )

        try:
            time.sleep(1.0)

            result = iii_client.trigger(
                {"function_id": "engine::functions::list", "payload": {}}
            )
            functions = [FunctionInfo(**f) for f in result.get("functions", [])]
            function_ids = [f.function_id for f in functions]

            assert "test::ew::public::echo" in function_ids
            assert "test::ew::meta-public" in function_ids

            assert "test::ew::valid-token-echo" not in function_ids
            assert "test::ew::private" not in function_ids
            assert "test::rbac-worker::auth" not in function_ids
        finally:
            iii_client.shutdown()

    def test_function_registration_prefix(self, iii_server):
        iii_client = register_worker(
            EW_URL,
            InitOptions(otel={"enabled": False}, headers={"x-test-token": "prefix-token"}),
        )

        try:
            iii_client.register_function(
                {"id": "prefixed-echo"},
                lambda data: {"echoed": data},
            )

            time.sleep(1.0)

            result = iii_server.trigger({
                "function_id": "test-prefix::prefixed-echo",
                "payload": {"msg": "prefix-test"},
            })

            assert result["echoed"]["msg"] == "prefix-test"
        finally:
            iii_client.shutdown()

    def test_forbidden_wrapped_as_typed_error(self, iii_server):
        """FORBIDDEN rejections surface as IIIForbiddenError with function_id set
        and the engine's remediation phrase in the message."""
        iii_client = register_worker(
            EW_URL,
            InitOptions(otel={"enabled": False}, headers={"x-test-token": "valid-token"}),
        )

        try:
            with pytest.raises(IIIForbiddenError) as excinfo:
                iii_client.trigger({
                    "function_id": "test::ew::private",
                    "payload": {},
                })

            err = excinfo.value
            assert isinstance(err, IIIInvocationError)  # base class
            assert err.code == "FORBIDDEN"
            assert err.function_id == "test::ew::private"
            assert "FORBIDDEN" in str(err)
            assert "test::ew::private" in str(err)
            # Remediation phrase from engine/src/engine/mod.rs:806
            assert "rbac.expose_functions" in str(err)
            # Guards against raw-dict regression (the Python equivalent of
            # Node's `[object Object]`).
            assert str(err) != repr({"code": "FORBIDDEN"})
        finally:
            iii_client.shutdown()

    def test_restricted_handler_happy_path_under_infra_carveout(self, iii_server):
        """Regression guard for the engine-side infrastructure carve-out.

        A worker whose `expose_functions` only lists `test::ew::*` must still
        be able to complete registration (engine::workers::register) and run
        SDK-transparent engine calls (engine::log::*, engine::baggage::*).
        If the carve-out ever regresses, connection setup FORBIDDENs here.
        """
        iii_client = register_worker(
            EW_URL,
            InitOptions(otel={"enabled": False}, headers={"x-test-token": "valid-token"}),
        )

        try:
            # Successful invocation proves handshake completed without tripping
            # FORBIDDEN on engine::workers::register (the transparent startup
            # trigger) — otherwise the worker would not be reachable.
            result = iii_client.trigger({
                "function_id": "test::ew::valid-token-echo",
                "payload": {"msg": "carveout-regression"},
            })
            assert result["valid_token"] is True
            assert result["echoed"]["msg"] == "carveout-regression"
        finally:
            iii_client.shutdown()

    # --- Infrastructure carve-out regression guards ---
    #
    # Lock in the engine-side INFRASTRUCTURE_FUNCTIONS carve-out end-to-end over
    # a real WebSocket. Previously a worker whose allowed_functions /
    # expose_functions did not cover `engine::*` IDs tripped FORBIDDEN the
    # moment a handler used the SDK logger — the reporter's original bug.
    # Paired with identical scenarios in
    # sdk/packages/node/iii/tests/rbac-workers.test.ts and
    # sdk/packages/rust/iii/tests/rbac_workers.rs.

    def test_infrastructure_logger_callable_from_user_handler(self, iii_server):
        """Real usage case: restricted worker's user handler calls the SDK
        logger during invocation.

        Handler runs under `allowed_functions: ['test::ew::valid-token-echo']`
        and internally hits `engine::log::info` — allowed only via the
        carve-out, not the allow-list. If the carve-out regresses, the nested
        invocation FORBIDDENs and the handler raises instead of returning
        ``{"logged": True}``.
        """
        iii_client = register_worker(
            EW_URL,
            InitOptions(otel={"enabled": False}, headers={"x-test-token": "valid-token"}),
        )

        try:
            def handler(data: dict) -> dict:
                # If the carve-out regresses, this nested trigger surfaces
                # IIIForbiddenError and the handler propagates it as a failure.
                iii_client.trigger({
                    "function_id": "engine::log::info",
                    "payload": {
                        "message": "carve-out regression guard: handler reached logger",
                        "data": {"input": data},
                    },
                })
                return {"logged": True, "echoed": data}

            # Use a test-unique function_id so this test doesn't clobber shared
            # registrations — a worker-client registration supersedes the
            # shared one, and the implicit unregister when the worker shuts
            # down would break every subsequent test that expects the shared
            # function to exist.
            iii_client.register_function("test::ew::carveout-logger-handler", handler)
            time.sleep(0.5)

            result = iii_server.trigger({
                "function_id": "test::ew::carveout-logger-handler",
                "payload": {"msg": "real-usage-case"},
            })
            assert result["logged"] is True
        finally:
            iii_client.shutdown()

    def test_infrastructure_logger_directly_callable(self, iii_server):
        """Direct variant: restricted worker invokes ``engine::log::info``
        straight from its client — mirrors a bootstrap script / CLI.

        ``engine::log::info`` is NOT in valid-token's allowed_functions, so
        a successful trigger here proves the carve-out path is reachable from
        the worker client's own ``trigger()`` method.
        """
        iii_client = register_worker(
            EW_URL,
            InitOptions(otel={"enabled": False}, headers={"x-test-token": "valid-token"}),
        )

        try:
            # No exception == carve-out is working. If this raises
            # IIIForbiddenError, the carve-out regressed.
            iii_client.trigger({
                "function_id": "engine::log::info",
                "payload": {"message": "carve-out direct invocation"},
            })
        finally:
            iii_client.shutdown()
