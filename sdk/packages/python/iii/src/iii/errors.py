from __future__ import annotations

from typing import Any


class IIIInvocationError(Exception):
    """Raised when an invocation dispatched by the SDK fails.

    Subclass by code:
      - ``IIIForbiddenError``  (``code == 'FORBIDDEN'``)
      - ``IIITimeoutError``    (``code == 'TIMEOUT'``)

    Catch the base to handle every rejection; catch a subclass to react to
    a specific category. ``except Exception`` continues to work because
    ``IIIInvocationError`` inherits from ``Exception``.

    Attributes are read-only after construction. ``stacktrace`` is the
    engine-side trace when the remote handler raised; it may include
    internal file paths and should not be surfaced to end users. ``str(err)``
    intentionally never includes the stacktrace.
    """

    def __init__(
        self,
        code: str,
        message: str,
        function_id: str | None = None,
        stacktrace: str | None = None,
        invocation_id: str | None = None,
    ) -> None:
        super().__init__(f"{code}: {message}")
        self.code = code
        self.message = message
        self.function_id = function_id
        self.stacktrace = stacktrace
        self.invocation_id = invocation_id


class IIIForbiddenError(IIIInvocationError):
    """Raised when RBAC denies an invocation. ``code == 'FORBIDDEN'``."""


class IIITimeoutError(IIIInvocationError):
    """Raised when an invocation exceeds its timeout. ``code == 'TIMEOUT'``."""


def _wrap_wire_error(
    error: Any,
    *,
    function_id: str | None,
    invocation_id: str | None,
) -> IIIInvocationError:
    """Convert a wire ``ErrorBody``-shaped dict into a typed exception.

    Dispatches to ``IIIForbiddenError`` / ``IIITimeoutError`` based on
    ``error['code']``. Malformed shapes (non-dict, missing fields, non-string
    values) fall back to ``IIIInvocationError(code='UNKNOWN', ...)`` so no
    rejection path prints as a raw dict repr.
    """
    if isinstance(error, dict):
        raw_code = error.get("code")
        code = raw_code if isinstance(raw_code, str) else "UNKNOWN"

        raw_message = error.get("message")
        message = raw_message if isinstance(raw_message, str) else "<no message>"

        raw_stacktrace = error.get("stacktrace")
        stacktrace = raw_stacktrace if isinstance(raw_stacktrace, str) else None

        cls: type[IIIInvocationError] = {
            "FORBIDDEN": IIIForbiddenError,
            "TIMEOUT": IIITimeoutError,
        }.get(code, IIIInvocationError)

        return cls(
            code=code,
            message=message,
            function_id=function_id,
            stacktrace=stacktrace,
            invocation_id=invocation_id,
        )

    return IIIInvocationError(
        code="UNKNOWN",
        message=str(error),
        function_id=function_id,
        invocation_id=invocation_id,
    )
