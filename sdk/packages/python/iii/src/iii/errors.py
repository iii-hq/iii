from __future__ import annotations

from typing import Any


class InvocationError(Exception):
    """Raised when an invocation dispatched by the SDK fails.

    Inspect ``err.code`` to react to a specific category (e.g.
    ``'FORBIDDEN'`` for RBAC denials, ``'TIMEOUT'`` for timeouts). Catch
    this class to handle every rejection. ``except Exception`` continues to
    work because ``InvocationError`` inherits from ``Exception``.

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


class StreamClosedError(InvocationError, RuntimeError):
    """A write or send on an :class:`~iii.OutputStream` that is closed or aborted
    (``code`` is ``'STREAM_CLOSED'``); also passing such a stream to a call.

    Also a :class:`RuntimeError`, as these errors were before they had a code.
    """

    def __init__(self, message: str) -> None:
        super().__init__("STREAM_CLOSED", message)


class StreamInUseError(InvocationError, RuntimeError):
    """A stream used where it can no longer be (``code`` is ``'STREAM_IN_USE'``): a
    second reader of an :class:`~iii.InputStream`, the same stream twice in one
    payload, a stream passed on after it was read to the end or already sent to
    another call, or a write to an :class:`~iii.OutputStream` passed on.

    Also a :class:`RuntimeError`, as these errors were before they had a code.
    """

    def __init__(self, message: str) -> None:
        super().__init__("STREAM_IN_USE", message)


class StreamAbortedError(InvocationError):
    """A stream ended in failure instead of a clean close (``code`` is ``'STREAM_ABORTED'``).

    Raised by a read of an :class:`~iii.InputStream` whose writer aborted it
    (its source failed, its handler raised, or its socket was lost), once the
    data sent before the abort has been read. A call rejects with it when a
    sink passed in its payload was aborted, even if the function succeeded.

    Attributes:
        reason: Why the stream was aborted, as the reader learned it. A 0.26
            engine relays an abort as ``channel writer aborted: closed with
            code <n>`` (or ``connection lost before close``), not the
            writer's own reason.
        close_code: The WebSocket close code the reader got (``1011`` for an
            abort the engine relayed), or ``None`` for an abort raised in
            this process.
    """

    def __init__(
        self,
        reason: str = "",
        *,
        close_code: int | None = None,
        function_id: str | None = None,
        invocation_id: str | None = None,
    ) -> None:
        detail = f"close {close_code}" if close_code is not None else ""
        if reason:
            detail = f"{detail}: {reason}" if detail else reason
        super().__init__(
            "STREAM_ABORTED",
            f"stream aborted ({detail})" if detail else "stream aborted",
            function_id=function_id,
            invocation_id=invocation_id,
        )
        self.reason = reason
        self.close_code = close_code


class RegistrationRejectedError(Exception):
    """Raised when the engine rejects this worker's registration.

    The engine pushes a ``registrationrejected`` message and closes the
    connection on a registration collision (e.g. another live worker already
    owns ``(namespace, worker_name)``). This is fatal: the SDK does not
    reconnect. Inspect the attributes to identify the conflict.
    """

    def __init__(
        self,
        code: str,
        namespace: str,
        worker_name: str,
        owner_worker_id: str,
    ) -> None:
        super().__init__(
            f"{code}: worker '{worker_name}' rejected in namespace "
            f"'{namespace}' (owned by '{owner_worker_id}')"
        )
        self.code = code
        self.namespace = namespace
        self.worker_name = worker_name
        self.owner_worker_id = owner_worker_id


def _wrap_wire_error(
    error: Any,
    *,
    function_id: str | None,
    invocation_id: str | None,
) -> InvocationError:
    """Convert a wire ``ErrorBody``-shaped dict into an ``InvocationError``.

    The ``code`` field distinguishes categories (e.g. ``'FORBIDDEN'``,
    ``'TIMEOUT'``). Malformed shapes (non-dict, missing fields, non-string
    values) fall back to ``code='UNKNOWN'`` so no rejection path prints as a
    raw dict repr.
    """
    if isinstance(error, dict):
        raw_code = error.get("code")
        code = raw_code if isinstance(raw_code, str) else "UNKNOWN"

        raw_message = error.get("message")
        message = raw_message if isinstance(raw_message, str) else "<no message>"

        raw_stacktrace = error.get("stacktrace")
        stacktrace = raw_stacktrace if isinstance(raw_stacktrace, str) else None

        return InvocationError(
            code=code,
            message=message,
            function_id=function_id,
            stacktrace=stacktrace,
            invocation_id=invocation_id,
        )

    return InvocationError(
        code="UNKNOWN",
        message=str(error),
        function_id=function_id,
        invocation_id=invocation_id,
    )


class ObserveRejection(Exception):
    """Rejects an observer with a code of its own.

    Raise it from an ``on_observer_added`` hook, at either level
    (``HTTP_ROUTE_CONFLICT``, ``INVALID_CRON_EXPRESSION``, ...); any other
    exception rejects the observer with ``OBSERVE_REJECTED``. Raised from an
    ``on_observation_deliver`` hook, its code is the error the caller gets. Also
    what ``ObserverHandle.ready()`` and ``Observer.accepted()`` raise when the
    observer turns ``error`` or ``disposed``, with the engine's or the hook's
    code (``INVALID_ARGS``, ``FORBIDDEN``, ``UNSUPPORTED``, ...).

    Examples:
        >>> @iii.on_observer_added("cron::on-tick")
        ... def start(obs):
        ...     if not is_valid_cron(obs.args["expression"]):
        ...         raise ObserveRejection("INVALID_CRON_EXPRESSION", "invalid day-of-week field")
    """

    def __init__(self, code: str, message: str) -> None:
        # Both in `args`, so the error pickles (and unpickles) whole.
        super().__init__(code, message)
        self.code = code
        self.message = message

    def __str__(self) -> str:
        return f"{self.code}: {self.message}"
