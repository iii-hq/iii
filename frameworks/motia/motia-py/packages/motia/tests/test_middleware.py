# motia/tests/test_middleware.py
"""Unit tests for HTTP middleware registration."""

import asyncio
from unittest.mock import MagicMock, patch

import pytest


@pytest.fixture
def mock_instance():
    mock = MagicMock()
    mock.register_function = MagicMock()
    mock.register_trigger = MagicMock()
    with patch("motia.runtime.get_instance", return_value=mock):
        yield mock


@pytest.fixture
def motia_cls():
    from motia import Motia

    return Motia


@pytest.fixture
def http_fn():
    from motia import http

    return http


def _run(coro):
    return asyncio.get_event_loop().run_until_complete(coro)


class TestMiddlewareRegistration:
    def test_registers_middleware_as_function_and_passes_ids_in_trigger_config(
        self, mock_instance, motia_cls, http_fn
    ):
        async def middleware(req, ctx, next_fn):
            await next_fn()

        async def handler(req, ctx):
            return {"status": 200, "body": None}

        motia = motia_cls()
        config = {"name": "test", "triggers": [http_fn("GET", "/test", middleware=[middleware])]}
        motia.add_step(config, "test.step.py", handler)

        # Middleware registered as a regular function
        mw_calls = [
            c for c in mock_instance.register_function.call_args_list if "middleware::0" in str(c)
        ]
        assert len(mw_calls) == 1
        mw_id = mw_calls[0][0][0]["id"]
        assert mw_id == "steps::test::trigger::http(GET /test)::middleware::0"

        # Trigger config includes middleware_function_ids
        trigger_call = mock_instance.register_trigger.call_args_list[-1]
        trigger_config = trigger_call[0][0]["config"]
        assert trigger_config["middleware_function_ids"] == [
            "steps::test::trigger::http(GET /test)::middleware::0"
        ]

    def test_middleware_wrapper_continue_on_next(self, mock_instance, motia_cls, http_fn):
        async def middleware(req, ctx, next_fn):
            await next_fn()
            return {"status": 200, "body": None}

        async def handler(req, ctx):
            return {"status": 200, "body": None}

        motia = motia_cls()
        config = {"name": "test", "triggers": [http_fn("GET", "/test", middleware=[middleware])]}
        motia.add_step(config, "test.step.py", handler)

        mw_calls = [
            c for c in mock_instance.register_function.call_args_list if "middleware::0" in str(c)
        ]
        mw_wrapper = mw_calls[0][0][1]

        engine_req = {
            "phase": "preHandler",
            "request": {"path_params": {}, "query_params": {}, "headers": {}, "method": "GET"},
            "context": {},
        }
        result = _run(mw_wrapper(engine_req))
        assert result == {"action": "continue"}

    def test_middleware_wrapper_short_circuit(self, mock_instance, motia_cls, http_fn):
        async def auth_middleware(req, ctx, next_fn):
            return {"status": 401, "body": {"error": "Unauthorized"}}

        motia = motia_cls()
        config = {"name": "test", "triggers": [http_fn("GET", "/admin", middleware=[auth_middleware])]}
        motia.add_step(config, "test.step.py", lambda r, c: None)

        mw_calls = [
            c for c in mock_instance.register_function.call_args_list if "middleware::0" in str(c)
        ]
        mw_wrapper = mw_calls[0][0][1]

        engine_req = {
            "phase": "preHandler",
            "request": {"path_params": {}, "query_params": {}, "headers": {}, "method": "GET"},
            "context": {},
        }
        result = _run(mw_wrapper(engine_req))
        assert result == {
            "action": "respond",
            "response": {"status_code": 401, "body": {"error": "Unauthorized"}, "headers": None},
        }

    def test_no_middleware_no_ids_in_config(self, mock_instance, motia_cls, http_fn):
        async def handler(req, ctx):
            return {"status": 200, "body": None}

        motia = motia_cls()
        config = {"name": "test", "triggers": [http_fn("GET", "/test")]}
        motia.add_step(config, "test.step.py", handler)

        trigger_call = mock_instance.register_trigger.call_args_list[-1]
        trigger_config = trigger_call[0][0]["config"]
        assert "middleware_function_ids" not in trigger_config

    def test_middleware_wrapper_void_returns_continue(self, mock_instance, motia_cls, http_fn):
        async def silent_middleware(req, ctx, next_fn):
            pass  # returns None, no next() call

        motia = motia_cls()
        config = {
            "name": "test",
            "triggers": [http_fn("GET", "/test", middleware=[silent_middleware])],
        }
        motia.add_step(config, "test.step.py", lambda r, c: None)

        mw_calls = [
            c for c in mock_instance.register_function.call_args_list if "middleware::0" in str(c)
        ]
        mw_wrapper = mw_calls[0][0][1]

        engine_req = {
            "phase": "preHandler",
            "request": {"path_params": {}, "query_params": {}, "headers": {}, "method": "GET"},
            "context": {},
        }
        result = _run(mw_wrapper(engine_req))
        assert result == {"action": "continue"}

    def test_middleware_wrapper_propagates_errors(self, mock_instance, motia_cls, http_fn):
        async def error_middleware(req, ctx, next_fn):
            raise ValueError("middleware exploded")

        motia = motia_cls()
        config = {
            "name": "test",
            "triggers": [http_fn("GET", "/test", middleware=[error_middleware])],
        }
        motia.add_step(config, "test.step.py", lambda r, c: None)

        mw_calls = [
            c for c in mock_instance.register_function.call_args_list if "middleware::0" in str(c)
        ]
        mw_wrapper = mw_calls[0][0][1]

        engine_req = {
            "phase": "preHandler",
            "request": {"path_params": {}, "query_params": {}, "headers": {}, "method": "GET"},
            "context": {},
        }
        with pytest.raises(ValueError, match="middleware exploded"):
            _run(mw_wrapper(engine_req))

    def test_multiple_middleware_sequential_ids(self, mock_instance, motia_cls, http_fn):
        async def mw1(req, ctx, next_fn):
            await next_fn()

        async def mw2(req, ctx, next_fn):
            await next_fn()

        motia = motia_cls()
        config = {"name": "test", "triggers": [http_fn("GET", "/test", middleware=[mw1, mw2])]}
        motia.add_step(config, "test.step.py", lambda r, c: None)

        mw0_calls = [
            c for c in mock_instance.register_function.call_args_list if "middleware::0" in str(c)
        ]
        mw1_calls = [
            c for c in mock_instance.register_function.call_args_list if "middleware::1" in str(c)
        ]
        assert len(mw0_calls) == 1
        assert len(mw1_calls) == 1

        trigger_call = mock_instance.register_trigger.call_args_list[-1]
        trigger_config = trigger_call[0][0]["config"]
        assert trigger_config["middleware_function_ids"] == [
            "steps::test::trigger::http(GET /test)::middleware::0",
            "steps::test::trigger::http(GET /test)::middleware::1",
        ]
