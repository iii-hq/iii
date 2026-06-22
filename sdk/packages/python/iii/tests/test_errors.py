"""Unit tests for the typed invocation error hierarchy. No engine required."""

from __future__ import annotations

import pytest

from iii import InvocationError
from iii.errors import _wrap_wire_error


class TestInvocationError:
    def test_exposes_all_fields(self) -> None:
        err = InvocationError(
            code="FORBIDDEN",
            message="function 'engine::functions::list' not allowed",
            function_id="engine::functions::list",
            stacktrace="trace here",
            invocation_id="inv-123",
        )
        assert isinstance(err, Exception)
        assert isinstance(err, InvocationError)
        assert err.code == "FORBIDDEN"
        assert err.message == "function 'engine::functions::list' not allowed"
        assert err.function_id == "engine::functions::list"
        assert err.stacktrace == "trace here"
        assert err.invocation_id == "inv-123"

    def test_str_is_code_colon_message(self) -> None:
        err = InvocationError(
            code="FORBIDDEN",
            message="function 'X' not allowed (add to rbac.expose_functions)",
            function_id="X",
        )
        assert str(err) == "FORBIDDEN: function 'X' not allowed (add to rbac.expose_functions)"

    def test_str_never_looks_like_raw_dict_repr(self) -> None:
        """Guards against the original Node [object Object] equivalent."""
        err = InvocationError(code="FORBIDDEN", message="nope")
        assert str(err) != "{'code': 'FORBIDDEN', 'message': 'nope'}"
        assert str(err) != repr({"code": "FORBIDDEN", "message": "nope"})

    def test_str_does_not_leak_stacktrace(self) -> None:
        """Stacktrace is opt-in via .stacktrace attribute; str/repr must not include it."""
        err = InvocationError(
            code="HANDLER",
            message="boom",
            stacktrace="/internal/path/secrets.py:line 42",
        )
        assert "/internal/path/secrets.py" not in str(err)
        assert "/internal/path/secrets.py" not in repr(err)

    def test_supports_optional_fields(self) -> None:
        err = InvocationError(code="TIMEOUT", message="gone")
        assert err.function_id is None
        assert err.stacktrace is None
        assert err.invocation_id is None
        assert str(err) == "TIMEOUT: gone"


class TestCodeDiscrimination:
    def test_categories_discriminated_by_code(self) -> None:
        """Categories are distinguished by ``code``, not by subclass."""
        for code in ("FORBIDDEN", "TIMEOUT", "UNKNOWN"):
            err = InvocationError(code=code, message="x")
            assert isinstance(err, InvocationError)
            assert isinstance(err, Exception)
            assert err.code == code

    def test_base_catches_every_category(self) -> None:
        for err in (
            InvocationError(code="FORBIDDEN", message="x"),
            InvocationError(code="TIMEOUT", message="x"),
            InvocationError(code="UNKNOWN", message="x"),
        ):
            try:
                raise err
            except InvocationError as got:
                assert got.code in {"FORBIDDEN", "TIMEOUT", "UNKNOWN"}

    def test_except_exception_still_works(self) -> None:
        """Migration guarantee: existing `except Exception:` handlers still catch."""
        try:
            raise InvocationError(code="FORBIDDEN", message="x")
        except Exception as got:
            assert isinstance(got, InvocationError)


class TestWrapWireError:
    def test_forbidden_dict_sets_forbidden_code(self) -> None:
        err = _wrap_wire_error(
            {"code": "FORBIDDEN", "message": "not allowed"},
            function_id="engine::functions::list",
            invocation_id="inv-1",
        )
        assert type(err) is InvocationError
        assert err.code == "FORBIDDEN"
        assert err.function_id == "engine::functions::list"
        assert err.invocation_id == "inv-1"

    def test_timeout_dict_sets_timeout_code(self) -> None:
        err = _wrap_wire_error(
            {"code": "TIMEOUT", "message": "gone"},
            function_id="api::slow",
            invocation_id=None,
        )
        assert type(err) is InvocationError
        assert err.code == "TIMEOUT"

    def test_unknown_code_falls_back_to_base(self) -> None:
        err = _wrap_wire_error(
            {"code": "BUSINESS_RULE", "message": "nope"},
            function_id=None,
            invocation_id=None,
        )
        assert type(err) is InvocationError
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
        assert isinstance(err, InvocationError)
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


class TestErrorsSubmodule:
    def test_subpath_import(self) -> None:
        from iii.errors import InvocationError as FromErrors

        assert FromErrors is InvocationError

    def test_removed_alias_not_at_root(self) -> None:
        import pytest

        import iii

        with pytest.raises(AttributeError):
            _ = iii.IIIInvocationError
