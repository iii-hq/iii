"""The current-invocation context.

Set around every handler and the function's ``on_observer_deliver``, remote
(``InvokeFunction``) and local (in-process dispatch), and read by outgoing
calls (``caller``, ``observe_depth``). It lives in a
:class:`contextvars.ContextVar`, so it follows asyncio tasks, and sync
handlers and hooks start their thread with :func:`contextvars.copy_context`
to carry it (with the OpenTelemetry context).
"""

from __future__ import annotations

import asyncio
import contextvars
import threading
from collections.abc import Callable
from dataclasses import dataclass
from typing import Any, TypeVar

T = TypeVar("T")


@dataclass(frozen=True)
class ActiveInvocation:
    """The invocation a handler runs in (internal; see :class:`InvocationContext`)."""

    function_id: str
    invocation_id: str
    observe_depth: int = 0
    #: ``{worker_id?, worker_name?, function_id?, invocation_id?, parent?}``
    caller: dict[str, Any] | None = None
    #: ``{observer_id, activation_id?}`` when this invocation is an activation.
    activation: dict[str, Any] | None = None
    #: The invocation's metadata sidecar.
    metadata: Any = None
    #: Local calls this one is nested in (the re-entrancy guard); 0 for a remote invocation.
    local_depth: int = 0


@dataclass(frozen=True)
class InvocationContext:
    """The invocation the current code runs in, in every handler, remote or local.

    Returned by ``iii.current_invocation()``.

    Attributes:
        function_id: The function whose handler is running.
        invocation_id: This invocation's id.
        observe_depth: Depth of the causal chain in observe hops (0 outside
            any activation).
        caller: Who started it: ``{worker_id, worker_name, function_id?,
            invocation_id?, parent?}``; ``None`` when unknown.
        activation: ``{observer_id, activation_id?}`` when this invocation is
            the activation of an observer's target.
    """

    function_id: str
    invocation_id: str
    observe_depth: int = 0
    caller: dict[str, Any] | None = None
    activation: dict[str, Any] | None = None


_CURRENT: contextvars.ContextVar[ActiveInvocation | None] = contextvars.ContextVar(
    "iii_current_invocation", default=None
)


def current_context() -> ActiveInvocation | None:
    return _CURRENT.get()


def set_context(ctx: ActiveInvocation | None) -> contextvars.Token[ActiveInvocation | None]:
    return _CURRENT.set(ctx)


def reset_context(token: contextvars.Token[ActiveInvocation | None]) -> None:
    try:
        _CURRENT.reset(token)
    except ValueError:
        # A handler coroutine closed outside its task (garbage-collected at
        # shutdown): its context is gone with it, nothing to restore.
        pass


def public_context(ctx: ActiveInvocation | None) -> InvocationContext | None:
    """The public view of an invocation (without the metadata sidecar)."""
    if ctx is None:
        return None
    return InvocationContext(
        function_id=ctx.function_id,
        invocation_id=ctx.invocation_id,
        observe_depth=ctx.observe_depth,
        caller=ctx.caller,
        activation=ctx.activation,
    )


def current_invocation() -> InvocationContext | None:
    """The invocation the calling code runs in (any handler, remote or
    local), or ``None`` outside every handler.

    Examples:
        >>> from iii import current_invocation
        >>> def handler(data):
        ...     inv = current_invocation()
        ...     return {"id": inv.invocation_id if inv else None}
    """
    return public_context(_CURRENT.get())


class HandlerExit(Exception):
    """A ``BaseException`` that is not an ``Exception`` (``SystemExit``,
    ``KeyboardInterrupt``, ...) raised by a handler, carried as an ordinary
    error so it cannot stop the SDK's event loop. The original is its
    ``__cause__``."""

    def __init__(self, error: BaseException) -> None:
        super().__init__(f"{type(error).__name__}: {error}")
        self.__cause__ = error


def guard_exit(error: BaseException) -> BaseException:
    """``error``, or a :class:`HandlerExit` for one an event loop must not see."""
    if isinstance(error, Exception) or isinstance(error, asyncio.CancelledError):
        return error
    return HandlerExit(error)


def original_error(error: BaseException, wrappers: tuple[type[BaseException], ...] = ()) -> BaseException:
    """The handler's own error under the SDK's wrappers (``HandlerExit`` and ``wrappers``)."""
    while isinstance(error, (HandlerExit, *wrappers)) and error.__cause__ is not None:
        error = error.__cause__
    return error


def run_in_thread(fn: Callable[..., T], *args: Any) -> asyncio.Future[T]:
    """Runs a sync callable on a fresh thread, in a copy of the caller's
    context (the current invocation and the OpenTelemetry context), and
    returns a future of the running loop that settles with its outcome.

    A raw thread rather than the loop's executor: a sync handler that blocks
    on a nested sync call would otherwise exhaust a bounded pool.
    """
    loop = asyncio.get_running_loop()
    future: asyncio.Future[T] = loop.create_future()
    ctx = contextvars.copy_context()

    def settle_result(result: T) -> None:
        if not future.done():
            future.set_result(result)

    def settle_error(error: BaseException) -> None:
        if not future.done():
            future.set_exception(error)

    def run() -> None:
        settle: Callable[[Any], None]
        try:
            value: Any = ctx.run(fn, *args)
            settle = settle_result
        except BaseException as error:
            # SystemExit / KeyboardInterrupt would stop the loop that awaits it
            value = guard_exit(error)
            settle = settle_error
        try:
            loop.call_soon_threadsafe(settle, value)
        except RuntimeError:
            pass  # the loop closed while the handler ran (shutdown): nobody waits for it

    threading.Thread(target=run, daemon=True).start()
    return future
