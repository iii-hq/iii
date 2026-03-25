"""Integration tests for Worker RBAC module."""

import os
import time

import pytest

from iii import InitOptions, register_worker

ENGINE_WS_URL = os.environ.get("III_URL", "ws://localhost:49199")
EW_URL = os.environ.get("III_RBAC_WORKER_URL", "ws://localhost:49135")

auth_calls: list[dict] = []


@pytest.fixture(scope="module")
def iii_server():
    """Server-side III client that registers auth, interceptor, and echo functions."""
    client = register_worker(ENGINE_WS_URL)

    def auth_handler(data):
        auth_calls.append(data)
        headers = data.get("headers", {})
        token = headers.get("x-test-token")

        if not token:
            return {
                "allowed_functions": [],
                "forbidden_functions": [],
                "context": {"role": "anonymous", "user_id": "anonymous"},
            }

        if token == "valid-token":
            return {
                "allowed_functions": ["test::ew::valid-token-echo"],
                "forbidden_functions": [],
                "context": {"role": "admin", "user_id": "user-1"},
            }

        if token == "restricted-token":
            return {
                "allowed_functions": [],
                "forbidden_functions": ["test::ew::echo"],
                "context": {"role": "restricted", "user_id": "user-2"},
            }

        raise Exception("invalid token")

    def interceptor_handler(data):
        function_id = data["function_id"]
        payload = data["payload"]
        context = data["context"]
        enriched = {**payload, "_intercepted": True, "_caller": context.get("user_id")}
        return client.trigger({"function_id": function_id, "payload": enriched})

    def echo_handler(data):
        return {"echoed": data}

    def valid_token_echo_handler(data):
        return {"echoed": data, "valid_token": True}

    def meta_public_handler(data):
        return {"meta_echoed": data}

    def private_handler(_data):
        return {"private": True}

    client.register_function({"id": "test::rbac-worker::auth"}, auth_handler)
    client.register_function({"id": "test::rbac-worker::middleware"}, interceptor_handler)
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
def _reset_auth_calls():
    auth_calls.clear()


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
            assert auth_calls[0]["headers"]["x-test-token"] == "valid-token"
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
