"""The host side of observe: per-function observer state, the observer hooks
(``on_observer_added`` / ``on_observer_removed`` at both levels), the
:class:`Observer` handed to them, and each call's delivery (``next``,
``on_observer_deliver``, keyed delivery).

Threading: the engine's messages are handled on the SDK loop, and the hooks
of one function run there one at a time, in the order the engine announced
the observers (sync hooks on a thread of their own). ``Observer.matches`` /
``reject`` / ``accepted``, ``observers()`` and ``is_observed()`` may be called
from any thread: the active set is an immutable tuple swapped under a lock,
and so is the keyed index.
"""

from __future__ import annotations

import asyncio
import inspect
import json
import logging
import threading
from collections.abc import Awaitable, Callable, Coroutine, Mapping, Sequence
from dataclasses import dataclass
from typing import Any, Literal, Protocol, Union

from .errors import InvocationError, ObserveRejection
from .filter import CompiledFilter, compile_filter
from .iii_types import ObserverInfoWire
from .invocation_context import run_in_thread
from .keyed import Keyed
from .observe_types import AddedHook, Call, DeliverHook, RemovedHook
from .observe_wire import report_caller
from .reporter import Reporter, ReportInit

log = logging.getLogger("iii.observe")

#: An ``on_observer_added`` (or ``on_observer_removed``) still running after this is ``OWNER_TIMEOUT``.
OBSERVE_HOOK_TIMEOUT_S = 30.0

#: The two levels a hook is set at: every function this worker hosts, or one function.
Level = Literal["worker", "function"]

#: A partial call (``{"input": ...}``) or a :class:`Call`, for ``matches`` and ``is_observed``.
CallLike = Union[Call[Any, Any], Mapping[str, Any]]

_PARTS = ("input", "output", "error", "caller", "metadata")


class ObserveHost(Protocol):
    """What the host side needs from the client."""

    reporter: Reporter

    def send_ack(
        self,
        function_id: str,
        namespace: str | None,
        observer_id: str,
        revision: int,
        error: dict[str, str] | None,
        on_sent: Callable[[], None] | None = None,
    ) -> None:
        """Sends an ``observerack`` when connected (thread-safe); ``on_sent``
        runs once the socket took it."""

    def added_hook(self, function_id: str, level: Level) -> AddedHook | None:
        """The ``on_observer_added`` set at ``level`` for ``function_id``."""

    def removed_hook(self, function_id: str, level: Level) -> RemovedHook | None:
        """The ``on_observer_removed`` set at ``level`` for ``function_id``."""

    def invoke_hook(self, hook: AddedHook | RemovedHook, obs: Observer) -> Awaitable[None]:
        """Runs a sync or async hook on the SDK loop (a sync one on its own thread)."""

    def hook_timeout_s(self) -> float: ...

    def trace_headers(self) -> tuple[str | None, str | None]:
        """``(traceparent, baggage)`` of the current trace context."""

    def call_soon(self, callback: Callable[[], None]) -> None:
        """Runs ``callback`` on the SDK loop (thread-safe)."""

    def on_sdk_thread(self) -> bool:
        """The caller runs on the SDK loop's thread."""

    def delivers_v2(self) -> bool:
        """The current connection's engine advertised ``observe.v2``."""


def is_async_callable(fn: Callable[..., Any]) -> bool:
    return inspect.iscoroutinefunction(fn) or inspect.iscoroutinefunction(getattr(fn, "__call__", None))


@dataclass
class _HookOutcome:
    ok: bool
    code: str = ""
    message: str = ""
    late: asyncio.Future[Any] | None = None


def _same_json(a: Any, b: Any) -> bool:
    try:
        return json.dumps(a, sort_keys=True) == json.dumps(b, sort_keys=True)
    except (TypeError, ValueError):
        return bool(a == b)


def _on_accepts(on: str, ok: bool) -> bool:
    if on == "complete":
        return True
    if on == "success":
        return ok
    if on == "error":
        return not ok
    return False


def _rejection_of(error: BaseException) -> tuple[str, str]:
    """The ``(code, message)`` an observer is rejected with for ``error``."""
    if isinstance(error, ObserveRejection):
        return error.code, error.message
    return "OBSERVE_REJECTED", str(error) or type(error).__name__


def call_doc(call: CallLike) -> dict[str, Any]:
    """What filters read of a call: ``{input, output | error, caller?, metadata?}``.
    A partial call carries the parts it has."""
    if isinstance(call, Mapping):
        return {part: call[part] for part in _PARTS if part in call}
    doc: dict[str, Any] = {"input": call.input}
    if call.status == "success":
        doc["output"] = call.output
    else:
        doc["error"] = call.error
    if call.caller is not None:
        doc["caller"] = call.caller
    if call.metadata is not None:
        doc["metadata"] = call.metadata
    return doc


def as_call(function_id: str, call: CallLike) -> Call[Any, Any]:
    """A :class:`Call` from a partial one (``{"input": ...}``), for key functions."""
    if isinstance(call, Call):
        return call
    status = call.get("status", "success")
    return Call(
        function_id=call.get("function_id", function_id),
        invocation_id=call.get("invocation_id", ""),
        status="error" if status == "error" else "success",
        input=call.get("input"),
        output=call.get("output"),
        error=call.get("error"),
        caller=call.get("caller"),
        metadata=call.get("metadata"),
    )


class Observer:
    """An observer as the worker that hosts its function sees it
    (``on_observer_added``, ``observers()``, a deliver hook's ``observers``):
    what was asked for. Never who observes, the observer's metadata or its
    policy.

    The object stays the same for the observer's whole lifetime, so a worker
    keeps it in its own index (route table, consumers, ...).

    Attributes:
        id: The observer's id.
        function_id: The observed function (one this worker hosts).
        args: The observer's args, validated against ``args_format`` by the engine.
        filters: The observer's filters, normalized; ``next`` and
            :meth:`matches` apply them.
        target: The target function id.
        capture: What travels to the target when iii delivers.
        on: ``success``, ``error`` or ``complete``.
    """

    def __init__(self, owner: FunctionObservers, info: ObserverInfoWire, filter_: CompiledFilter, revision: int):
        self._owner = owner
        self.id: str = info.id
        self.function_id: str = owner.function_id
        self.args: dict[str, Any] = info.args
        self.filters: dict[str, Any] = info.filters
        self.target: str = info.target
        self.capture: list[str] = list(info.capture)
        self.on: str = info.on
        self._filter = filter_
        #: The revision it was acked with (echoed by a late rejection).
        self._revision = revision
        #: In the active set: delivery reaches it.
        self._active = False
        #: Accepted: its ``on_observer_removed`` runs when it goes.
        self._accepted = False
        #: The levels whose ``on_observer_added`` resolved for it.
        self._setup: set[str] = set()
        #: The keys it is filed under on a keyed function.
        self._keys: tuple[str, ...] = ()
        #: A rejection (late, or while its hooks ran), re-acked until the engine removes the observer.
        self._rejection: dict[str, str] | None = None
        #: ``on_observer_added`` is running for it: a rejection now wins once it returns.
        self._adding = False
        self._settled = threading.Condition()
        self._acked = False
        self._refusal: ObserveRejection | None = None
        self._waiters: list[Callable[[], None]] = []

    def _update(self, info: ObserverInfoWire, filter_: CompiledFilter) -> None:
        """Takes the engine's current view of a kept observer."""
        self.filters = info.filters
        self._filter = filter_
        self.target = info.target
        self.capture = list(info.capture)
        self.on = info.on

    def matches(self, call: CallLike) -> bool:
        """Whether ``call`` passes this observer's ``on`` and filters: what
        ``next`` checks, for a deliver hook that calls targets itself.

        ``call`` is a :class:`Call` or a partial one (``{"input": ...}``);
        ``on`` is checked when it has a ``status``.
        """
        status = call.get("status") if isinstance(call, Mapping) else call.status
        if status is not None and not _on_accepts(self.on, status == "success"):
            return False
        return self._filter.matches(call_doc(call))

    def accepted(self, timeout: float | None = None) -> None:
        """Blocks until the SDK has acked this observer: a call through it
        made afterwards reaches an active observer. Use it to send a new
        observer its initial state, outside its ``on_observer_added`` (which
        runs before the ack). Use :meth:`accepted_async` in async code.

        Raises:
            ObserveRejection: The observer was rejected, or removed, first.
            TimeoutError: ``timeout`` elapsed first.
            RuntimeError: Called from the SDK's event-loop thread.
        """
        if self._owner.host.on_sdk_thread():
            raise RuntimeError(
                "Cannot call Observer.accepted() from the event loop thread. Use await obs.accepted_async() instead."
            )
        with self._settled:
            if not self._settled.wait_for(lambda: self._acked or self._refusal is not None, timeout):
                raise TimeoutError(f"observer {self.id} was not accepted after {timeout}s")
            refusal = None if self._acked else self._refusal
        if refusal is not None:
            raise refusal

    async def accepted_async(self) -> None:
        """Waits until the SDK has acked this observer (on any event loop).

        Raises:
            ObserveRejection: The observer was rejected, or removed, first.
        """
        loop = asyncio.get_running_loop()
        done: asyncio.Future[None] = loop.create_future()

        def wake() -> None:
            def settle() -> None:
                if not done.done():
                    done.set_result(None)

            try:
                loop.call_soon_threadsafe(settle)
            except RuntimeError:
                pass  # that loop closed: nobody waits

        with self._settled:
            settled = self._acked or self._refusal is not None
            if not settled:
                self._waiters.append(wake)
        if not settled:
            await done
        if not self._acked and self._refusal is not None:
            raise self._refusal

    def _settle(self, acked: bool, refusal: ObserveRejection | None = None) -> None:
        with self._settled:
            if self._acked or self._refusal is not None:
                return
            self._acked = acked
            self._refusal = refusal
            self._settled.notify_all()
            waiters, self._waiters = self._waiters, []
        for wake in waiters:
            wake()

    def _mark_acked(self) -> None:
        self._settle(True)

    def _refuse(self, code: str, message: str) -> None:
        self._settle(False, ObserveRejection(code, message))

    def reject(self, code: str, message: str) -> None:
        """Late rejection of an active observer (e.g. after a config reload).

        The observer leaves the active set at once; ``on_observer_removed``
        runs when the engine removes it. Called while its
        ``on_observer_added`` runs, the rejection wins once the hook returns.
        """
        self._owner.reject(self, code, message)

    def to_dict(self) -> dict[str, Any]:
        return {
            "id": self.id,
            "function_id": self.function_id,
            "args": self.args,
            "filters": self.filters,
            "target": self.target,
            "capture": self.capture,
            "on": self.on,
        }

    def __repr__(self) -> str:
        return f"Observer(id={self.id!r}, function_id={self.function_id!r}, target={self.target!r}, args={self.args!r})"


class FunctionObservers:
    """One registered function's observers, as the engine pushed them
    (``functionregistered``, ``observeradded``, ``observerremoved``). Hooks
    run one at a time, in announcement order, on the SDK loop."""

    def __init__(
        self,
        function_id: str,
        local_dispatch_option: bool,
        has_handler: bool,
        host: ObserveHost,
        *,
        private: bool = False,
        deliver: DeliverHook | None = None,
    ) -> None:
        self.function_id = function_id
        self.host = host
        #: Calls of this worker's own pass the input and the result by reference.
        self.private = private
        self._local_dispatch_option = local_dispatch_option
        self._has_handler = has_handler
        #: ``functionregistered`` arrived (cleared by a rejection).
        self.confirmed = False
        #: The connection (epoch) the last ``functionregistered`` arrived on.
        self.confirmed_epoch = -1
        #: The connection (epoch) the ``registerfunction`` went out on.
        self.sent_epoch = -1
        #: In-process dispatch: ``functionregistered.local_dispatch`` ∧ the option. Survives a disconnect.
        self.local_dispatch = False
        #: The namespace the engine registered the function in.
        self.namespace: str | None = None
        #: The highest revision received on this connection.
        self.revision = -1
        #: The connection (epoch) on which the engine refused the function: on
        #: it, a flush counts its reports ``INACTIVE`` (also those of a call
        #: already running). A later connection may accept the replayed
        #: registration, so its reports wait for the answer;
        #: ``functionregistered`` clears it.
        self.released_epoch: int | None = None
        self._known: dict[str, Observer] = {}
        #: The active observers: one immutable tuple, swapped on change.
        self.compiled: tuple[Observer, ...] = ()
        #: The ``on_observer_deliver`` hook; ``None`` is iii's delivery.
        self.deliver: DeliverHook | None = None
        #: ``(keyed, index)`` when ``deliver`` is a :class:`Keyed`: key → observers.
        self._keyed: tuple[Keyed, dict[str, tuple[Observer, ...]]] | None = None
        self._lock = threading.Lock()
        self._tail: asyncio.Task[None] | None = None
        self._disposed = False
        #: Bumped when the function is released (rejection, unregister): an
        #: add whose hook returns after it is undone.
        self._releases = 0
        self.set_deliver(deliver)

    def is_observed(self, call: CallLike | None = None) -> bool:
        """Any active observer; with a call on a keyed function, any observer
        filed under one of the call's keys."""
        if not self.compiled:
            return False
        keyed = self._keyed
        if call is None or keyed is None:
            return True
        hook, index = keyed
        return any(key in index for key in hook.call_keys(as_call(self.function_id, call)))

    def observers(self) -> list[Observer]:
        return list(self.compiled)

    # ── the deliver hook and the keyed index (any thread) ──────────────────

    def set_deliver(self, hook: DeliverHook | None) -> None:
        """Takes a new ``on_observer_deliver``. A :class:`Keyed` one files the
        active observers now; an observer whose key function raises is
        rejected."""
        failed: list[tuple[Observer, BaseException]] = []
        with self._lock:
            self.deliver = hook
            keyed = hook if isinstance(hook, Keyed) else None
            if keyed is (self._keyed[0] if self._keyed is not None else None):
                return
            if keyed is None:
                self._keyed = None
                return
            index: dict[str, tuple[Observer, ...]] = {}
            for obs in self.compiled:
                try:
                    obs._keys = keyed.observer_keys(obs)
                except Exception as error:
                    failed.append((obs, error))
                    continue
                for key in obs._keys:
                    index[key] = (*index.get(key, ()), obs)
            self._keyed = (keyed, index)
        for obs, failure in failed:
            obs.reject(*_rejection_of(failure))

    def keyed_select(self, call: Call[Any, Any]) -> list[Observer]:
        """The observers filed under any of ``call``'s keys, each once."""
        keyed = self._keyed
        if keyed is None:
            return list(self.compiled)
        hook, index = keyed
        seen: set[str] = set()
        chosen: list[Observer] = []
        for key in hook.call_keys(call):
            for obs in index.get(key, ()):
                if obs.id not in seen:
                    seen.add(obs.id)
                    chosen.append(obs)
        return chosen

    # ── engine messages (SDK loop) ─────────────────────────────────────────

    def on_function_registered(
        self, namespace: str, revision: int, local_dispatch: bool, observers: list[ObserverInfoWire], epoch: int
    ) -> None:
        """``functionregistered``: always accepted; replaces the set."""
        if self._disposed:
            return
        self.confirmed = True
        self.released_epoch = None
        self.local_dispatch = local_dispatch and self._local_dispatch_option and self._has_handler
        self.namespace = namespace
        self.revision = revision
        releases = self._releases

        async def apply() -> None:
            try:
                await self._apply_set(observers, revision, namespace, releases)
            finally:
                # Reports held for this function go once the engine has the acks
                # of its replayed set: before them its observers are still
                # pending (INACTIVE). Also when the diff failed: the engine
                # confirmed the function.
                self.confirmed_epoch = epoch
                self.host.reporter.flush()

        self._enqueue(apply)

    def on_observer_added(self, namespace: str, revision: int, info: ObserverInfoWire) -> None:
        if self._disposed or revision <= self.revision:
            return
        self.revision = revision
        releases = self._releases
        self._enqueue(lambda: self._add(info, revision, namespace, releases, fresh=True))

    def on_observer_removed(self, revision: int, observer_id: str) -> None:
        if self._disposed or revision <= self.revision:
            return
        self.revision = revision
        self._enqueue(lambda: self._remove(observer_id))

    def on_registration_rejected(self, epoch: int) -> None:
        """The engine refused the function on connection ``epoch``: no local
        dispatch, no observers, and its held reports are released (they could
        never be confirmed)."""
        self.confirmed = False
        self.confirmed_epoch = -1
        self.sent_epoch = -1
        self.local_dispatch = False
        self.released_epoch = epoch
        self.host.reporter.discard(self.function_id)
        self._drop_all()

    def dispose(self) -> None:
        """The function was unregistered (any thread): local dispatch stops at
        once, and ``on_observer_removed`` runs for every accepted observer."""
        self._disposed = True
        self.confirmed = False
        self.confirmed_epoch = -1
        self.local_dispatch = False
        self.host.reporter.discard(self.function_id)
        self.host.call_soon(self._drop_all)

    def catch_up(self, level: Level) -> None:
        """An ``on_observer_added`` was set at ``level`` (any thread): it runs,
        in hook order, for every active observer that was accepted without
        one at that level."""
        self.host.call_soon(lambda: self._enqueue(lambda: self._catch_up(level)))

    def _drop_all(self) -> None:
        """Releases every observer (SDK loop). Delivery stops at once; the
        removals run through the hook queue, after the adds queued before, so
        an observer still in ``on_observer_added`` is removed too (its add is
        undone when the hook returns)."""
        self._releases += 1
        with self._lock:
            for obs in self.compiled:
                obs._active = False
            self.compiled = ()
            if self._keyed is not None:
                self._keyed = (self._keyed[0], {})

        async def remove_all() -> None:
            for observer_id in list(self._known):
                await self._remove(observer_id)

        self._enqueue(remove_all)

    def _enqueue(self, task: Callable[[], Awaitable[None]]) -> None:
        previous = self._tail

        async def run() -> None:
            if previous is not None:
                try:
                    await previous
                except BaseException:
                    pass
            try:
                await task()
            except Exception:
                log.exception("Observer hook failed for %s", self.function_id)

        self._tail = asyncio.get_running_loop().create_task(run())

    # ── the active set ─────────────────────────────────────────────────────

    def _activate(self, obs: Observer) -> bool:
        """Puts the observer in the active set (and the keyed index), unless
        it was rejected (``reject`` takes the same lock, so it cannot slip in
        between) or its keys cannot be computed (it is rejected then)."""
        with self._lock:
            if obs._rejection is not None:
                return False
            if obs._active:
                return True
            if self._keyed is not None:
                keyed, index = self._keyed
                try:
                    obs._keys = keyed.observer_keys(obs)
                except Exception as error:
                    code, message = _rejection_of(error)
                    obs._rejection = {"code": code, "message": message}
                    return False
                for key in obs._keys:
                    index[key] = (*index.get(key, ()), obs)
            obs._active = True
            obs._accepted = True
            self.compiled = (*self.compiled, obs)
            return True

    def _deactivate(self, obs: Observer) -> None:
        with self._lock:
            if not obs._active:
                return
            obs._active = False
            self.compiled = tuple(o for o in self.compiled if o is not obs)
            if self._keyed is not None:
                index = self._keyed[1]
                for key in obs._keys:
                    rest = tuple(o for o in index.get(key, ()) if o is not obs)
                    if rest:
                        index[key] = rest
                    else:
                        index.pop(key, None)

    def _ack(
        self,
        obs: Observer,
        revision: int,
        namespace: str | None,
        error: dict[str, str] | None = None,
    ) -> None:
        on_sent = obs._mark_acked if error is None else None
        self.host.send_ack(self.function_id, namespace, obs.id, revision, error, on_sent)

    def _released_since(self, releases: int) -> bool:
        """The function was released (or unregistered) after a message was queued."""
        return self._disposed or self._releases != releases

    async def _apply_set(self, observers: list[ObserverInfoWire], revision: int, namespace: str, releases: int) -> None:
        """Diffs the engine's full set against what this SDK holds. Nothing is
        done for a set that arrived before the function was released."""
        if self._released_since(releases):
            return
        incoming = {info.id for info in observers}
        for observer_id in list(self._known):
            if observer_id not in incoming:
                await self._remove(observer_id)
        for info in observers:
            if self._released_since(releases):
                return
            existing = self._known.get(info.id)
            if existing is not None and _same_json(existing.args, info.args):
                self._keep(existing, info, revision, namespace)
            else:
                if existing is not None:
                    await self._remove(info.id)
                await self._add(info, revision, namespace, releases, fresh=False)

    def _keep(self, obs: Observer, info: ObserverInfoWire, revision: int, namespace: str) -> None:
        """A kept observer: its hooks already ran; take the engine's filters
        and ack it again (with its rejection, if it has one)."""
        obs._revision = revision
        if obs._rejection is not None:
            self._ack(obs, revision, namespace, obs._rejection)
            return
        filter_ = obs._filter
        if not _same_json(obs.filters, info.filters):
            try:
                filter_ = compile_filter(info.filters, trusted=True)
            except Exception as error:
                self._deactivate(obs)
                self._ack(obs, revision, namespace, {"code": "INVALID_FILTER", "message": str(error)})
                return
        obs._update(info, filter_)
        if self._activate(obs):
            self._ack(obs, revision, namespace)
        else:
            self._ack(obs, revision, namespace, obs._rejection)

    async def _add(self, info: ObserverInfoWire, revision: int, namespace: str, releases: int, *, fresh: bool) -> None:
        """An added observer: the worker's ``on_observer_added``, then the
        function's, then the active set, then the ack. A hook that raises (or
        runs past the timeout) rejects the observer, and the
        ``on_observer_removed`` of each level whose hook already resolved
        runs. ``releases`` is the release count when the message was queued:
        an add queued before the function was released never runs its hooks."""
        if self._released_since(releases):
            return
        existing = self._known.get(info.id)
        if existing is not None:
            if fresh and existing._rejection is not None:
                # A fresh `observeradded` for an observer rejected late: a new start.
                await self._remove(info.id)
                if self._released_since(releases):
                    return  # released while its removal hook ran
            else:
                self._keep(existing, info, revision, namespace)
                return
        try:
            filter_ = compile_filter(info.filters, trusted=True)
        except Exception as error:
            self.host.send_ack(
                self.function_id, namespace, info.id, revision, {"code": "INVALID_FILTER", "message": str(error)}
            )
            return
        obs = Observer(self, info, filter_, revision)
        obs._adding = True
        setup: list[Level] = []
        levels: tuple[Level, ...] = ("worker", "function")
        for level in levels:
            hook = self.host.added_hook(self.function_id, level)
            if hook is None:
                continue
            outcome = await self._run_hook(hook, obs)
            if not outcome.ok:
                obs._adding = False
                self._ack(obs, revision, namespace, {"code": outcome.code, "message": outcome.message})
                obs._refuse(outcome.code, outcome.message)
                if outcome.late is not None:
                    self._undo_when_done(obs, level, outcome.late)
                await self._unwind(obs, setup)
                return
            setup.append(level)
        released = self._released_since(releases)
        with self._lock:
            # Known before it stops being "adding": a `reject()` from another
            # thread right now finds it one way or the other.
            if not released:
                self._known[obs.id] = obs
            obs._adding = False
        if released:
            # The function was released while the hooks ran: undo them, no ack.
            obs._refuse("OBSERVER_REMOVED", f"{self.function_id} was released while observer {obs.id} was added")
            await self._unwind(obs, setup)
            return
        obs._setup.update(setup)
        # In the active set before the ack: the first call after `active` is
        # delivered to it. Refused when it was rejected (`obs.reject`, also
        # while its hooks ran, or its keys failed): the rejection wins.
        if self._activate(obs):
            self._ack(obs, revision, namespace)
            return
        rejection = obs._rejection or {"code": "OBSERVE_REJECTED", "message": "rejected"}
        self._ack(obs, revision, namespace, rejection)
        obs._refuse(rejection["code"], rejection["message"])
        obs._setup.clear()
        await self._unwind(obs, setup)

    def _undo_when_done(self, obs: Observer, level: Level, task: asyncio.Future[Any]) -> None:
        """A timed-out ``on_observer_added`` may still resolve: its level's
        ``on_observer_removed`` runs once it does."""

        def undo(done: asyncio.Future[Any]) -> None:
            if done.cancelled() or done.exception() is not None:
                return  # it failed after all: nothing was set up
            self._enqueue(lambda: self._run_removed(obs, (level,)))

        task.add_done_callback(undo)

    async def _unwind(self, obs: Observer, levels: list[Level]) -> None:
        """Tears down the levels whose ``on_observer_added`` resolved for an
        observer that was not accepted: the function's, then the worker's."""
        await self._run_removed(obs, tuple(reversed(levels)))

    async def _remove(self, observer_id: str) -> None:
        obs = self._known.pop(observer_id, None)
        if obs is None:
            return
        self._deactivate(obs)
        obs._refuse("OBSERVER_REMOVED", f"observer {obs.id} of {self.function_id} was removed")
        if not obs._accepted:
            return
        obs._accepted = False
        obs._setup.clear()
        await self._run_removed(obs, ("function", "worker"))

    async def _run_removed(self, obs: Observer, levels: tuple[Level, ...]) -> None:
        for level in levels:
            hook = self.host.removed_hook(self.function_id, level)
            if hook is None:
                continue
            outcome = await self._run_hook(hook, obs)
            if not outcome.ok:
                log.error(
                    "on_observer_removed failed for observer %s of %s: %s", obs.id, self.function_id, outcome.message
                )

    async def _catch_up(self, level: Level) -> None:
        hook = self.host.added_hook(self.function_id, level)
        if hook is None:
            return
        behind = [obs for obs in self.compiled if level not in obs._setup]
        if not behind:
            return
        log.warning(
            "on_observer_added was set for %s while %d of its observers were active without it; it ran for each "
            "of them now. Set hooks before registering the function so they run before an observer is accepted.",
            self.function_id,
            len(behind),
        )
        for obs in behind:
            if not obs._active:
                continue
            outcome = await self._run_hook(hook, obs)
            if outcome.ok:
                obs._setup.add(level)
            else:
                obs.reject(outcome.code, outcome.message)

    async def _run_hook(self, hook: AddedHook | RemovedHook, obs: Observer) -> _HookOutcome:
        task = asyncio.ensure_future(self.host.invoke_hook(hook, obs))
        timeout = self.host.hook_timeout_s()
        done, _ = await asyncio.wait({task}, timeout=timeout)
        if not done:
            return _HookOutcome(
                False,
                "OWNER_TIMEOUT",
                f"observer hook of {self.function_id} still running after {int(timeout * 1000)}ms",
                late=task,
            )
        if task.cancelled():
            return _HookOutcome(False, "OBSERVE_REJECTED", "the observer hook was cancelled")
        error = task.exception()
        if error is None:
            return _HookOutcome(True)
        return _HookOutcome(False, *_rejection_of(error))

    def reject(self, obs: Observer, code: str, message: str) -> None:
        with self._lock:
            if obs._adding:
                # The add acks with it once the hooks return.
                obs._rejection = {"code": code, "message": message}
                return
            if self._known.get(obs.id) is not obs:
                return
            # Kept on the observer: if this ack is lost with the connection,
            # the replayed set gets it again (`_keep`).
            obs._rejection = {"code": code, "message": message}
        self._deactivate(obs)
        self._ack(obs, obs._revision, self.namespace, obs._rejection)


class Delivery:
    """One call's delivery: the ``next`` calls its handler's call goes
    through (iii's own, keyed, or a deliver hook's), and what they matched.

    Each observer is delivered at most once per call. The ``next`` calls given
    the handler's call unchanged pool what they deliver; each ``next`` call
    given a changed call reports its own item. A local call pushes its items
    to the reporter; a routed one collects them (:attr:`items`) for the host
    to send ahead of the answer, and its pooled ids ride the answer's
    ``observed`` when the answer is the handler's own outcome.
    """

    def __init__(
        self,
        owner: FunctionObservers,
        call: Call[Any, Any],
        handler_error: BaseException | None,
        *,
        routed: bool,
        started_at_ms: float,
        duration_ms: float,
        observe_depth: int,
    ) -> None:
        self.owner = owner
        #: The handler's call.
        self.call = call
        #: What the handler raised (``call.error`` is its wire form).
        self.handler_error = handler_error
        self._routed = routed
        self._v2 = owner.host.delivers_v2()
        self._started_at_ms = started_at_ms
        self._duration_ms = duration_ms
        self._observe_depth = observe_depth
        self._lock = threading.Lock()
        self._delivered: set[str] = set()
        self._pooled: list[Observer] = []
        self._pooled_trace: tuple[str | None, str | None] = (None, None)
        self._settled = False
        #: A routed call's items, sent ahead of its answer.
        self.items: list[tuple[ReportInit, set[str]]] = []

    def _unchanged(self, call: Call[Any, Any]) -> bool:
        own = self.call
        return call is own or (
            call.input is own.input
            and call.status == own.status
            and call.output is own.output
            and call.error is own.error
            and call.caller is own.caller
            and call.metadata is own.metadata
        )

    def deliver(self, call: Call[Any, Any], observers: Sequence[Observer]) -> None:
        """iii's delivery of ``call`` to the observers passed whose ``on`` and
        filters match it, less those this call already delivered to."""
        with self._lock:
            if self._settled and self._routed:
                self.owner.host.reporter.drop(self.owner.function_id, "LATE_DELIVERY")
                return
            doc: dict[str, Any] | None = None
            ok = call.status == "success"
            matched: list[Observer] = []
            for obs in observers:
                if obs.id in self._delivered or not _on_accepts(obs.on, ok):
                    continue
                if doc is None:
                    doc = call_doc(call)
                if obs._filter.matches(doc):
                    self._delivered.add(obs.id)
                    matched.append(obs)
            if not matched:
                return
            if not self._settled and self._unchanged(call):
                if not self._pooled:
                    self._pooled_trace = self.owner.host.trace_headers()
                self._pooled.extend(matched)
                return
            item = self._item(call, matched, self.owner.host.trace_headers())
            if self._routed:
                self.items.append(item)
                return
        self.owner.host.reporter.push(*item)

    def outcome(self, call: Call[Any, Any]) -> Any:
        """What ``next`` gives back: the call's output, or its error raised
        (the handler's own exception for the handler's call)."""
        if call.status != "error":
            return call.output
        if self.handler_error is not None and self._unchanged(call):
            raise self.handler_error
        body = call.error or {}
        raise InvocationError(
            code=str(body.get("code", "invocation_failed")),
            message=str(body.get("message", "")),
            function_id=self.owner.function_id,
            stacktrace=body.get("stacktrace") if isinstance(body.get("stacktrace"), str) else None,
            invocation_id=self.call.invocation_id,
        )

    def next_sync(self, call: Call[Any, Any], observers: Sequence[Observer]) -> Any:
        """``next`` for a sync deliver hook."""
        self.deliver(call, observers)
        return self.outcome(call)

    async def next_async(self, call: Call[Any, Any], observers: Sequence[Observer]) -> Any:
        """``next`` for an async deliver hook."""
        self.deliver(call, observers)
        return self.outcome(call)

    def _own(self) -> tuple[bool, Any]:
        if self.call.status == "error":
            return False, self.handler_error
        return True, self.call.output

    def is_own_outcome(self, ok: bool, value: Any) -> bool:
        """The answer is the handler's: its output, or the error it raised."""
        if self.call.status == "success":
            return ok and value is self.call.output
        return not ok and value is self.handler_error

    async def run_async(self, hook: DeliverHook | None, observers: Sequence[Observer]) -> tuple[bool, Any]:
        """Delivers the handler's call and returns ``(ok, value)``: what the
        caller gets back, or the error it gets. A sync hook runs on a thread of
        its own."""
        if hook is None or isinstance(hook, Keyed):
            return self._deliver_builtin(hook, observers)
        try:
            if is_async_callable(hook):
                result = await hook(self.call, observers, self.next_async)
            else:
                result = await run_in_thread(hook, self.call, observers, self.next_sync)
            if inspect.isawaitable(result):
                result = await result
            return True, result
        except asyncio.CancelledError:
            raise
        except BaseException as error:
            # SystemExit / KeyboardInterrupt from a hook fail this call, never the SDK loop.
            return False, error

    def run_sync(
        self,
        hook: DeliverHook | None,
        observers: Sequence[Observer],
        run_coroutine: Callable[[Coroutine[Any, Any, Any]], Any],
    ) -> tuple[bool, Any]:
        """:meth:`run_async` on the calling thread: a sync hook runs here, an
        async one through ``run_coroutine`` (on the SDK loop)."""
        if hook is None or isinstance(hook, Keyed):
            return self._deliver_builtin(hook, observers)
        try:
            if is_async_callable(hook):
                result = run_coroutine(hook(self.call, observers, self.next_async))
            else:
                result = hook(self.call, observers, self.next_sync)
                if inspect.iscoroutine(result):
                    result = run_coroutine(result)
            return True, result
        except BaseException as error:
            return False, error

    def _deliver_builtin(self, keyed: Keyed | None, observers: Sequence[Observer]) -> tuple[bool, Any]:
        if keyed is not None:
            try:
                observers = self.owner.keyed_select(self.call)
            except Exception as error:
                return False, error
        self.deliver(self.call, observers)
        return self._own()

    def settle(self, own_outcome: bool) -> list[str] | None:
        """The hook settled. Returns the ids for a routed answer's
        ``observed`` (the pooled ids, when the answer is the handler's own
        outcome); otherwise the pooled ids become one item that carries the
        handler's call."""
        with self._lock:
            self._settled = True
            pooled, self._pooled = self._pooled, []
            if not pooled:
                return None
            if self._routed and own_outcome:
                return [obs.id for obs in pooled]
            item = self._item(self.call, pooled, self._pooled_trace)
            if self._routed:
                self.items.append(item)
                return None
        self.owner.host.reporter.push(*item)
        return None

    def _item(
        self, call: Call[Any, Any], matched: list[Observer], trace: tuple[str | None, str | None]
    ) -> tuple[ReportInit, set[str]]:
        values: dict[str, Any] = {"input": call.input}
        if call.status == "success":
            values["output"] = call.output
        else:
            values["error"] = call.error
        caller = report_caller(call.caller)
        if caller is not None:
            values["caller"] = caller
        if call.metadata is not None:
            values["metadata"] = call.metadata
        captures: set[str] = set()
        for obs in matched:
            captures.update(obs.capture)
        init = ReportInit(
            function_id=self.owner.function_id,
            namespace=self.owner.namespace,
            invocation_id=self.call.invocation_id,
            matched=[obs.id for obs in matched],
            status="success" if call.status == "success" else "error",
            values=values,
            started_at_ms=self._started_at_ms,
            duration_ms=self._duration_ms,
            observe_depth=self._observe_depth,
            traceparent=trace[0],
            baggage=trace[1],
            output_is_input=self._v2 and call.status == "success" and call.output is call.input,
        )
        return init, captures

