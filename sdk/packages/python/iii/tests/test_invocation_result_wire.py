"""Wire shape of InvocationResult replies (MOT-4732).

A handler that returns ``None`` answered ``null``; the engine forwards that to
the caller as ``"result": null``. Dropping the key would reach a TypeScript
caller as ``undefined`` and break the natural ``=== null`` check.
"""

from __future__ import annotations

from iii.iii import III
from iii.iii_types import InvocationResultMessage


def _wire(msg: InvocationResultMessage) -> dict[str, object]:
    return III.__new__(III)._to_dict(msg)


def test_none_result_is_sent_as_null() -> None:
    wire = _wire(
        InvocationResultMessage(invocation_id="inv-1", function_id="kv::get", result=None)
    )

    assert "result" in wire
    assert wire["result"] is None
    assert "error" not in wire


def test_value_result_is_untouched() -> None:
    wire = _wire(
        InvocationResultMessage(invocation_id="inv-1", function_id="kv::get", result={"a": 1})
    )

    assert wire["result"] == {"a": 1}


def test_error_reply_has_no_result_key() -> None:
    wire = _wire(
        InvocationResultMessage(
            invocation_id="inv-1",
            function_id="kv::get",
            error={"code": "invocation_failed", "message": "boom"},
        )
    )

    assert "result" not in wire
    assert wire["error"] == {"code": "invocation_failed", "message": "boom"}
