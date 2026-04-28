"""Unit tests for the typed invocation error hierarchy. No engine required."""

from __future__ import annotations

import pytest

from iii import IIIForbiddenError, IIIInvocationError, IIITimeoutError
from iii.errors import _wrap_wire_error


class TestIIIInvocationError:
    def test_exposes_all_fields(self) -> None:
        err = IIIInvocationError(
            code="FORBIDDEN",
            message="function 'engine::functions::list' not allowed",
            function_id="engine::functions::list",
            stacktrace="trace here",
            invocation_id="inv-123",
        )
        assert isinstance(err, Exception)
        assert isinstance(err, IIIInvocationError)
        assert err.code == "FORBIDDEN"
        assert err.message == "function 'engine::functions::list' not allowed"
        assert err.function_id == "engine::functions::list"
        assert err.stacktrace == "trace here"
        assert err.invocation_id == "inv-123"

    def test_str_is_code_colon_message(self) -> None:
        err = IIIInvocationError(
            code="FORBIDDEN",
            message="function 'X' not allowed (add to rbac.expose_functions)",
            function_id="X",
        )
        assert str(err) == "FORBIDDEN: function 'X' not allowed (add to rbac.expose_functions)"

    def test_str_never_looks_like_raw_dict_repr(self) -> None:
        """Guards against the original Node [object Object] equivalent."""
        err = IIIInvocationError(code="FORBIDDEN", message="nope")
        assert str(err) != "{'code': 'FORBIDDEN', 'message': 'nope'}"
        assert str(err) != repr({"code": "FORBIDDEN", "message": "nope"})

    def test_str_does_not_leak_stacktrace(self) -> None:
        """Stacktrace is opt-in via .stacktrace attribute; str/repr must not include it."""
        err = IIIInvocationError(
            code="HANDLER",
            message="boom",
            stacktrace="/internal/path/secrets.py:line 42",
        )
        assert "/internal/path/secrets.py" not in str(err)
        assert "/internal/path/secrets.py" not in repr(err)

    def test_supports_optional_fields(self) -> None:
        err = IIIInvocationError(code="TIMEOUT", message="gone")
        assert err.function_id is None
        assert err.stacktrace is None
        assert err.invocation_id is None
        assert str(err) == "TIMEOUT: gone"


class TestSubclassHierarchy:
    def test_forbidden_is_invocation_error(self) -> None:
        err = IIIForbiddenError(code="FORBIDDEN", message="x")
        assert isinstance(err, IIIInvocationError)
        assert isinstance(err, IIIForbiddenError)
        assert isinstance(err, Exception)

    def test_timeout_is_invocation_error(self) -> None:
        err = IIITimeoutError(code="TIMEOUT", message="x")
        assert isinstance(err, IIIInvocationError)
        assert isinstance(err, IIITimeoutError)
        assert isinstance(err, Exception)

    def test_except_ordering_catches_subclass_first(self) -> None:
        """`except IIIForbiddenError` fires before `except IIIInvocationError`."""
        caught: str | None = None
        try:
            raise IIIForbiddenError(code="FORBIDDEN", message="x")
        except IIIForbiddenError:
            caught = "forbidden"
        except IIIInvocationError:
            caught = "base"
        assert caught == "forbidden"

    def test_base_catches_every_subclass(self) -> None:
        for err in (
            IIIForbiddenError(code="FORBIDDEN", message="x"),
            IIITimeoutError(code="TIMEOUT", message="x"),
            IIIInvocationError(code="UNKNOWN", message="x"),
        ):
            try:
                raise err
            except IIIInvocationError as got:
                assert got.code in {"FORBIDDEN", "TIMEOUT", "UNKNOWN"}

    def test_except_exception_still_works(self) -> None:
        """Migration guarantee: existing `except Exception:` handlers still catch."""
        try:
            raise IIIForbiddenError(code="FORBIDDEN", message="x")
        except Exception as got:
            assert isinstance(got, IIIInvocationError)


class TestWrapWireError:
    def test_forbidden_dict_dispatches_to_forbidden_error(self) -> None:
        err = _wrap_wire_error(
            {"code": "FORBIDDEN", "message": "not allowed"},
            function_id="engine::functions::list",
            invocation_id="inv-1",
        )
        assert isinstance(err, IIIForbiddenError)
        assert err.code == "FORBIDDEN"
        assert err.function_id == "engine::functions::list"
        assert err.invocation_id == "inv-1"

    def test_timeout_dict_dispatches_to_timeout_error(self) -> None:
        err = _wrap_wire_error(
            {"code": "TIMEOUT", "message": "gone"},
            function_id="api::slow",
            invocation_id=None,
        )
        assert isinstance(err, IIITimeoutError)
        assert err.code == "TIMEOUT"

    def test_unknown_code_falls_back_to_base(self) -> None:
        err = _wrap_wire_error(
            {"code": "BUSINESS_RULE", "message": "nope"},
            function_id=None,
            invocation_id=None,
        )
        assert type(err) is IIIInvocationError
        assert err.code == "BUSINESS_RULE"

    def test_stacktrace_propagated_when_string(self) -> None:
        err = _wrap_wire_error(
            {"code": "HANDLER", "message": "boom", "stacktrace": "trace"},
            function_id=None,
            invocation_id=None,
        )
        assert err.stacktrace == "trace"

    @pytest.mark.parametrize(
        "bad_error",
        [
            None,
            "a plain string",
            42,
            {},
            {"code": 123, "message": "x"},
            {"code": "X"},
            {"message": "no code"},
            {"code": "X", "message": None},
        ],
    )
    def test_malformed_wire_errors_never_produce_raw_repr(self, bad_error: object) -> None:
        """Guards against stringified-dict regression for every pathological shape."""
        err = _wrap_wire_error(bad_error, function_id="fn", invocation_id=None)
        assert isinstance(err, IIIInvocationError)
        assert str(err).startswith(("UNKNOWN:", "X:", "123:"))
        assert "{'" not in str(err), f"dict repr leaked into message: {err!s}"
        assert "': " not in str(err), f"dict repr leaked into message: {err!s}"

    def test_non_string_stacktrace_ignored(self) -> None:
        err = _wrap_wire_error(
            {"code": "X", "message": "m", "stacktrace": 42},
            function_id=None,
            invocation_id=None,
        )
        assert err.stacktrace is None
