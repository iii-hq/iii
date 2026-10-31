"""The observing side of observe: the handle ``iii.observe`` returns, kept for
replay until it is disposed."""

from __future__ import annotations

import asyncio
import logging
import threading
from collections import deque
from collections.abc import Callable
from typing import cast

from .errors import ObserveRejection
from .iii_types import ObserveMessage
from .observe_types import ObserveReason, ObserverState

log = logging.getLogger("iii.observe")

StateListener = Callable[[ObserverState, "ObserveReason | None"], None]

_STATES = frozenset({"pending", "active", "suspended", "error", "disposed"})


def to_observer_state(state: str) -> ObserverState:
    """A wire state this SDK does not know (a newer engine) reads as ``pending``."""
    return cast(ObserverState, state if state in _STATES else "pending")


class ObserverHandle:
    """What ``iii.observe`` returns, synchronously.

    The observer is replayed with the same id on every reconnect until it is
    disposed. State listeners are called in order, on a callback thread of the
    SDK (never on the event loop that reads the engine's frames), so they may
    call the sync API.

    Attributes:
        id: The observer's id (stable across replays).
    """

    def __init__(
        self,
        observer_id: str,
        message: ObserveMessage,
        on_dispose: Callable[[ObserverHandle], None],
        sdk_thread: Callable[[], bool],
        schedule: Callable[[Callable[[], None]], None],
    ) -> None:
        self.id = observer_id
        #: What ``observe`` sends, replayed on every connection.
        self._message = message
        self._on_dispose = on_dispose
        self._sdk_thread = sdk_thread
        self._state: ObserverState = "pending"
        self._reason: ObserveReason | None = None
        self._listeners: list[StateListener] = []
        self._schedule = schedule
        self._undelivered: deque[tuple[ObserverState, ObserveReason | None]] = deque()
        self._delivering = False
        self._changed = threading.Condition()
        #: Sent on some connection: the engine may hold it, so disposing it needs an ``unobserve``.
        self._sent = False
        #: The connection (``III._connection_epoch``) it was last sent on, and
        #: whether that connection's ``observeresult`` for it arrived.
        self._sent_epoch = -1
        self._answered = False
        #: The engine's ``disposed`` for it arrived: it holds nothing there.
        self._engine_disposed = False
        #: Its ``dispose()`` released the id (once, whichever thread called it).
        self._disposed_by_worker = False

    @property
    def state(self) -> ObserverState:
        """``pending``, ``active``, ``suspended``, ``error`` or ``disposed``."""
        return self._state

    @property
    def reason(self) -> ObserveReason | None:
        """Why the observer is in its state (``error``, ``suspended``, ``disposed``)."""
        return self._reason

    def _settlement(self) -> tuple[bool, ObserveRejection | None]:
        """``(settled, error)``: active → ``(True, None)``; error / disposed →
        ``(True, rejection)``; otherwise still waiting."""
        if self._state == "active":
            return True, None
        if self._state in ("error", "disposed"):
            reason = self._reason or {
                "code": "OBSERVE_FAILED" if self._state == "error" else "UNOBSERVED",
                "message": f"observer {self.id} is {self._state}",
            }
            return True, ObserveRejection(reason["code"], reason["message"])
        return False, None

    def ready(self, timeout: float | None = None) -> None:
        """Blocks until the observer is ``active``. Use :meth:`ready_async` in async code.

        Args:
            timeout: Seconds to wait; ``None`` waits until the observer settles.

        Raises:
            ObserveRejection: The observer turned ``error`` or ``disposed``
                first; ``code`` is the engine's (``INVALID_ARGS``, ``FORBIDDEN``,
                the rejection of the observed function's worker, ``UNSUPPORTED``, ...).
            TimeoutError: ``timeout`` elapsed first.
            RuntimeError: Called from the SDK's event-loop thread.
        """
        if self._sdk_thread():
            raise RuntimeError(
                "Cannot call ObserverHandle.ready() from the event loop thread. Use await handle.ready_async() instead."
            )
        with self._changed:
            if not self._changed.wait_for(lambda: self._settlement()[0], timeout):
                raise TimeoutError(f"observer {self.id} is still {self._state} after {timeout}s")
            _, error = self._settlement()
        if error is not None:
            raise error

    async def ready_async(self) -> None:
        """Waits until the observer is ``active`` (on any event loop).

        Raises:
            ObserveRejection: The observer turned ``error`` or ``disposed`` first.
        """
        settled, error = self._settlement()
        if not settled:
            loop = asyncio.get_running_loop()
            done: asyncio.Future[None] = loop.create_future()

            def settle() -> None:
                if not done.done():
                    done.set_result(None)

            def wake(_state: ObserverState, _reason: ObserveReason | None) -> None:
                if self._settlement()[0]:
                    loop.call_soon_threadsafe(settle)

            off = self.on_state_change(wake)
            try:
                if not self._settlement()[0]:
                    await done
            finally:
                off()
            settled, error = self._settlement()
        if error is not None:
            raise error

    def on_state_change(self, listener: StateListener) -> Callable[[], None]:
        """Calls ``listener(state, reason)`` on every state change, in order,
        on the SDK's callback thread. Returns the unsubscribe function."""
        self._listeners.append(listener)

        def off() -> None:
            try:
                self._listeners.remove(listener)
            except ValueError:
                pass

        return off

    def dispose(self) -> None:
        """Removes the observer (``unobserve``). It is not replayed on reconnect."""
        if self._state == "disposed":
            return
        self._on_dispose(self)
        self._set_state("disposed", {"code": "UNOBSERVED", "message": "disposed by the observing worker"})

    async def dispose_async(self) -> None:
        """The async form of :meth:`dispose` (it does not wait for the engine)."""
        self.dispose()

    def _set_state(self, state: ObserverState, reason: ObserveReason | None = None) -> None:
        """A state from the engine (``observeresult``, ``observerstatechanged``) or the SDK."""
        with self._changed:
            if self._state == "disposed":
                return
            changed = state != self._state or reason != self._reason
            self._state = state
            self._reason = reason
            self._changed.notify_all()
            if not changed:
                return
            # Delivered later, in order: a listener never runs on the read path
            # (or inside dispose()), so it may call the client freely.
            self._undelivered.append((state, reason))
            if self._delivering:
                return
            self._delivering = True
        self._schedule(self._deliver)

    def _deliver(self) -> None:
        while True:
            with self._changed:
                if not self._undelivered:
                    self._delivering = False
                    return
                state, reason = self._undelivered.popleft()
            for listener in list(self._listeners):
                try:
                    listener(state, reason)
                except Exception:
                    log.exception("Observer %s state listener raised", self.id)

    def __repr__(self) -> str:
        return f"ObserverHandle(id={self.id!r}, state={self._state!r})"
