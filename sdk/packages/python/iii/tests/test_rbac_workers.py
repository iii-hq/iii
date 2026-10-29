"""Integration tests for Worker RBAC module."""

import os
import time

import pytest
from iii_helpers.worker_connection_manager import (
    AuthInput,
    AuthResult,
    OnFunctionRegistrationInput,
    OnFunctionRegistrationResult,
)

from iii import (
    InitOptions,
    InvocationError,
    MiddlewareFunctionInput,
    register_worker,
)

ENGINE_WS_URL = os.environ.get("III_URL", "ws://localhost:49199")
EW_URL = os.environ.get("III_RBAC_WORKER_URL", "ws://localhost:49135")

auth_calls: list[AuthInput] = []
function_reg_calls: list[OnFunctionRegistrationInput] = []


@pytest.fixture(scope="module")
def iii_server():
    """Server-side III client that registers auth, middleware, and echo functions."""
    # Explicit name: this fixture stays connected for the whole module while each
    # test connects its own client, and the engine allows only one live worker per
    # (namespace, name) -- default-named workers would conflict.
    client = register_worker(ENGINE_WS_URL, InitOptions(worker_name="rbac-test-server"))

    def auth_handler(data: dict) -> dict:
        auth_input = AuthInput(**data)
        auth_calls.append(auth_input)
        token = auth_input.headers.get("x-test-token")

        if not token:
            return AuthResult(
                allowed_functions=[],
                forbidden_functions=[],
                context={"role": "anonymous", "user_id": "anonymous"},
            ).model_dump()

        if token == "valid-token":
            return AuthResult(
                allowed_functions=["test::ew::valid-token-echo"],
                forbidden_functions=[],
                context={"role": "admin", "user_id": "user-1"},
            ).model_dump()

        if token == "restricted-token":
            return AuthResult(
                allowed_functions=[],
                forbidden_functions=["test::ew::echo"],
                context={"role": "restricted", "user_id": "user-2"},
            ).model_dump()

        if token == "prefix-token":
            return AuthResult(
                allowed_functions=[],
                forbidden_functions=[],
                context={"role": "prefixed", "user_id": "user-prefix"},
                function_registration_prefix="test-prefix",
            ).model_dump()

        raise Exception("invalid token")

    def middlware_handler(data: dict) -> dict:
        mid = MiddlewareFunctionInput(**data)
        enriched = {**mid.payload, "_intercepted": True, "_caller": mid.context.get("user_id")}
        return client.call({"function_id": mid.function_id, "payload": enriched})

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
        function_reg_calls.append(reg_input)
        if reg_input.function_id.startswith("denied::"):
            raise Exception("denied function registration")
        return OnFunctionRegistrationResult(
            function_id=reg_input.function_id,
        ).model_dump(exclude_none=True)

    client.register_function("test::rbac-worker::auth", auth_handler)
    client.register_function("test::rbac-worker::middleware", middlware_handler)
    client.register_function("test::rbac-worker::on-function-reg", on_function_reg_handler)
    client.register_function("test::ew::public::echo", echo_handler)
    client.register_function("test::ew::valid-token-echo", valid_token_echo_handler)
    client.register_function(
        "test::ew::meta-public",
        meta_public_handler,
        metadata={"ew_public": True},
    )
    client.register_function("test::ew::private", private_handler)

    time.sleep(1.0)
    yield client
    client.shutdown()


@pytest.fixture(autouse=True)
def _reset_calls():
    auth_calls.clear()
    function_reg_calls.clear()


class TestRbacWorkers:
    """RBAC Workers"""

    def test_should_return_auth_result_for_valid_token(self, iii_server):
        iii_client = register_worker(
            EW_URL,
            InitOptions(
                otel={"enabled": False},
                headers={"x-test-token": "valid-token"},
                worker_name="rbac-should-return-auth-result-for-valid-token",
            ),
        )

        try:
            result = iii_client.call({
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
            InitOptions(
                otel={"enabled": False},
                headers={"x-test-token": "valid-token"},
                worker_name="rbac-should-return-error-for-private-function",
            ),
        )

        try:
            with pytest.raises(Exception):
                iii_client.call({
                    "function_id": "test::ew::private",
                    "payload": {"msg": "hello"},
                })
        finally:
            iii_client.shutdown()

    def test_should_return_forbidden_functions_for_restricted_token(self, iii_server):
        iii_client = register_worker(
            EW_URL,
            InitOptions(
                otel={"enabled": False},
                headers={"x-test-token": "restricted-token"},
                worker_name="rbac-should-return-forbidden-functions-for-restricted-token",
            ),
        )

        try:
            with pytest.raises(Exception):
                iii_client.call({
                    "function_id": "test::ew::echo",
                    "payload": {"msg": "hello"},
                })
        finally:
            iii_client.shutdown()

    def test_the_function_registration_hook_sees_private_observable_and_args_format(self, iii_server):
        iii_client = register_worker(
            EW_URL,
            InitOptions(
                otel={"enabled": False},
                headers={"x-test-token": "valid-token"},
                worker_name="rbac-registration-hook-sees-observe-options",
            ),
        )

        try:
            args_format = {"type": "object", "properties": {"expression": {"type": "string"}}}
            iii_client.register_function(
                "rbac-hook::source", lambda tick: tick, private=True, args_format=args_format
            )
            iii_client.register_function("rbac-hook::sealed", lambda data: data, observable=False)

            deadline = time.time() + 5
            while len(function_reg_calls) < 2 and time.time() < deadline:
                time.sleep(0.05)

            by_id = {c.function_id: c for c in function_reg_calls}
            source, sealed = by_id["rbac-hook::source"], by_id["rbac-hook::sealed"]
            assert (source.private, source.observable, source.args_format) == (True, True, args_format)
            assert (sealed.private, sealed.observable, sealed.args_format) == (False, False, None)
            assert source.context["user_id"] == "user-1"
        finally:
            iii_client.shutdown()

    def test_should_deny_function_registration_via_hook(self, iii_server):
        iii_client = register_worker(
            EW_URL,
            InitOptions(
                otel={"enabled": False},
                headers={"x-test-token": "valid-token"},
                worker_name="rbac-should-deny-function-registration-via-hook",
            ),
        )

        try:
            iii_client.register_function(
                "denied::blocked-fn",
                lambda _data: {"should": "not reach"},
            )

            time.sleep(1.0)

            with pytest.raises(Exception):
                iii_client.call({
                    "function_id": "denied::blocked-fn",
                    "payload": {},
                })
        finally:
            iii_client.shutdown()

    def test_function_discovery_only_returns_allowed_for_valid_token(self, iii_server):
        iii_client = register_worker(
            EW_URL,
            InitOptions(
                otel={"enabled": False},
                headers={"x-test-token": "valid-token"},
                worker_name="rbac-function-discovery-only-returns-allowed-for-valid-token",
            ),
        )

        try:
            time.sleep(1.0)

            result = iii_client.call(
                {"function_id": "engine::functions::list", "payload": {}}
            )
            functions = result.get("functions", [])
            function_ids = [f["function_id"] for f in functions]

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
            InitOptions(
                otel={"enabled": False},
                headers={"x-test-token": "restricted-token"},
                worker_name="rbac-function-discovery-only-returns-exposed-for-restricted-token",
            ),
        )

        try:
            time.sleep(1.0)

            result = iii_client.call(
                {"function_id": "engine::functions::list", "payload": {}}
            )
            functions = result.get("functions", [])
            function_ids = [f["function_id"] for f in functions]

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
            InitOptions(
                otel={"enabled": False},
                headers={"x-test-token": "prefix-token"},
                worker_name="rbac-function-registration-prefix",
            ),
        )

        try:
            iii_client.register_function(
                "prefixed-echo",
                lambda data: {"echoed": data},
            )

            time.sleep(1.0)

            result = iii_server.call({
                "function_id": "test-prefix::prefixed-echo",
                "payload": {"msg": "prefix-test"},
            })

            assert result["echoed"]["msg"] == "prefix-test"
        finally:
            iii_client.shutdown()

    def test_forbidden_wrapped_as_typed_error(self, iii_server):
        """FORBIDDEN rejections surface as InvocationError (code='FORBIDDEN')
        with function_id set and the engine's remediation phrase in the message."""
        iii_client = register_worker(
            EW_URL,
            InitOptions(
                otel={"enabled": False},
                headers={"x-test-token": "valid-token"},
                worker_name="rbac-forbidden-wrapped-as-typed-error",
            ),
        )

        try:
            with pytest.raises(InvocationError) as excinfo:
                iii_client.call({
                    "function_id": "test::ew::private",
                    "payload": {},
                })

            err = excinfo.value
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
            InitOptions(
                otel={"enabled": False},
                headers={"x-test-token": "valid-token"},
                worker_name="rbac-restricted-handler-happy-path-under-infra-carveout",
            ),
        )

        try:
            # Successful invocation proves handshake completed without tripping
            # FORBIDDEN on engine::workers::register (the transparent startup
            # call) — otherwise the worker would not be reachable.
            result = iii_client.call({
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
            InitOptions(
                otel={"enabled": False},
                headers={"x-test-token": "valid-token"},
                worker_name="rbac-infrastructure-logger-callable-from-user-handler",
            ),
        )

        try:
            def handler(data: dict) -> dict:
                # If the carve-out regresses, this nested call surfaces
                # InvocationError (code='FORBIDDEN') and the handler propagates it as a failure.
                iii_client.call({
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

            result = iii_server.call({
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
        a successful call here proves the carve-out path is reachable from
        the worker client's own ``call()`` method.
        """
        iii_client = register_worker(
            EW_URL,
            InitOptions(
                otel={"enabled": False},
                headers={"x-test-token": "valid-token"},
                worker_name="rbac-infrastructure-logger-directly-callable",
            ),
        )

        try:
            # No exception == carve-out is working. If this raises
            # InvocationError (code='FORBIDDEN'), the carve-out regressed.
            iii_client.call({
                "function_id": "engine::log::info",
                "payload": {"message": "carve-out direct invocation"},
            })
        finally:
            iii_client.shutdown()
