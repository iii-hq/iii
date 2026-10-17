"""The owner side of observe (sdk.md §1, §4.1–§4.3): per-function observer
state, the observe hooks, the :class:`Observer` handed to them, and the
reports of local invocations and ``obs.emit`` events.

Threading: the engine's messages are handled on the SDK loop, and the hooks
of one function run there one at a time, in revision order (sync hooks on a
thread of their own). ``Observer.emit`` / ``matches`` / ``call`` and
``FunctionRef.is_observed()`` / ``observers()`` may be called from any thread:
the active set is an immutable tuple swapped under a lock.
"""

from __future__ import annotations

import asyncio
import json
import logging
import threading
import time
import uuid
from collections.abc import Awaitable, Callable, Mapping
from dataclasses import dataclass
from typing import Any, Literal, Protocol

from .errors import InvocationError, ObserveRejection
from .filter import CompiledFilter, compile_filter
from .iii_types import ObserverInfoWire
from .invocation_context import ActiveInvocation, current_context
from .observe_types import ObserveHook, ObserveHooks
from .observe_wire import strip_caller
from .reporter import Reporter, ReportInit

log = logging.getLogger("iii.observe")

#: CONTRACT D11: ``on_observer_added`` (and ``on_observer_removed``) still running after this is ``OWNER_TIMEOUT``.
OBSERVE_HOOK_TIMEOUT_S = 30.0


class ObserveHost(Protocol):
    """What the owner side needs from the client."""

    reporter: Reporter

    def identity(self) -> dict[str, str]:
        """This worker as the engine knows it: ``{worker_id?, worker_name}``."""

    def send_ack(
        self, function_id: str, namespace: str | None, observer_id: str, revision: int, error: dict[str, str] | None
    ) -> None:
        """Sends an ``observerack`` when connected (thread-safe)."""

    def call_target(self, target: str, observer_id: str, payload: Any, timeout_ms: int | None) -> Any:
        """``obs.call`` from a thread: a remote invocation of the target marked with the activation."""

    def call_target_async(
        self, target: str, observer_id: str, payload: Any, timeout_ms: int | None
    ) -> Awaitable[Any]: ...

    def invoke_hook(self, hook: ObserveHook, obs: Observer) -> Awaitable[None]:
        """Runs a sync or async hook on the SDK loop (a sync one on its own thread)."""

    def hook_timeout_s(self) -> float: ...

    def trace_headers(self) -> tuple[str | None, str | None]:
        """``(traceparent, baggage)`` of the current trace context."""

    def call_soon(self, callback: Callable[[], None]) -> None:
        """Runs ``callback`` on the SDK loop (thread-safe)."""


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


class Observer:
    """An observer as the function's owner sees it (``on_observer_added``,
    ``FunctionRef.observers()``): its args, filters, target, capture and
    ``on``. Never its metadata, policy or the observing worker.

    The object stays the same for the observer's whole lifetime, so owners
    keep it in their own index (expression → observers, route table, ...).

    Attributes:
        id: The observer's id.
        function_id: The observed function (the one this worker hosts).
        args: The observer's args, validated against ``args_format`` by the engine.
        filters: The observer's filters, normalized; the SDK applies them in
            ``emit`` / ``call``.
        target: The target function id.
        capture: What travels to the target (auto and ``emit``).
        on: ``success``, ``error`` or ``complete`` (auto only).
        legacy: ``{trigger_type, config}`` when the engine's legacy shim
            created the observer from an old ``register_trigger``.
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
        self.legacy: dict[str, Any] | None = (
            {"trigger_type": info.legacy.trigger_type, "config": info.legacy.config} if info.legacy else None
        )
        self._filter = filter_
        #: The revision it was acked with (echoed by a late rejection).
        self._revision = revision
        #: In the active set: ``emit`` / ``call`` deliver to it.
        self._active = False
        #: A late rejection (``reject``), re-acked until the engine removes the observer.
        self._rejection: dict[str, str] | None = None
        #: ``on_observer_added`` is running for it: a rejection now wins once it returns.
        self._adding = False

    def _update(self, info: ObserverInfoWire, filter_: CompiledFilter) -> None:
        """Takes the engine's current view of a kept observer."""
        self.filters = info.filters
        self._filter = filter_
        self.target = info.target
        self.capture = list(info.capture)
        self.on = info.on

    def matches(self, doc: Mapping[str, Any]) -> bool:
        """Evaluates the observer's filters on a document
        ``{input?, output?, error?, caller?, metadata?}``."""
        return self._filter.matches(doc)

    def emit(self, event: Any) -> bool:
        """Owner delivery, fire-and-forget: hands ``event`` to this observer.

        Returns ``False`` and sends nothing when its filters reject the event,
        or when the observer is no longer active.

        Raises:
            InvocationError: ``NOT_OWNER_DELIVERED`` on an auto function.
        """
        return self._owner.emit(self, event)

    def call(self, payload: Any, *, timeout_ms: int | None = None) -> Any:
        """Owner delivery, request/response: invokes the target with
        ``payload`` and returns its result. Blocks the calling thread; use
        :meth:`call_async` in async code.

        Raises:
            InvocationError: ``FILTERS_NOT_MATCHED`` (nothing is sent) when the
                filters reject the payload, ``OBSERVER_NOT_ACTIVE`` once the
                observer is removed, ``NOT_OWNER_DELIVERED`` on an auto
                function, or the engine's / target's error.
        """
        self._owner.check_call(self, payload)
        return self._owner.host.call_target(self.target, self.id, payload, timeout_ms)

    async def call_async(self, payload: Any, *, timeout_ms: int | None = None) -> Any:
        """The async form of :meth:`call`."""
        self._owner.check_call(self, payload)
        return await self._owner.host.call_target_async(self.target, self.id, payload, timeout_ms)

    def reject(self, code: str, message: str) -> None:
        """Late rejection of an active observer (e.g. after a config reload).

        The observer leaves the active set at once; ``on_observer_removed``
        runs when the engine removes it.
        """
        self._owner.reject(self, code, message)

    def to_dict(self) -> dict[str, Any]:
        out: dict[str, Any] = {
            "id": self.id,
            "function_id": self.function_id,
            "args": self.args,
            "filters": self.filters,
            "target": self.target,
            "capture": self.capture,
            "on": self.on,
        }
        if self.legacy is not None:
            out["legacy"] = self.legacy
        return out

    def __repr__(self) -> str:
        return f"Observer(id={self.id!r}, function_id={self.function_id!r}, target={self.target!r}, args={self.args!r})"


class FunctionObservers:
    """One registered function's observers, as the engine pushed them
    (``functionregistered``, ``observeradded``, ``observerremoved``). Hooks
    run one at a time, in revision order, on the SDK loop."""

    def __init__(
        self,
        function_id: str,
        hooks: ObserveHooks | None,
        local_dispatch_option: bool,
        has_handler: bool,
        host: ObserveHost,
    ) -> None:
        self.function_id = function_id
        self.hooks = hooks
        self.host = host
        self.delivery: Literal["auto", "owner"] = "owner" if hooks is not None and hooks.delivery == "owner" else "auto"
        self._local_dispatch_option = local_dispatch_option
        self._has_handler = has_handler
        #: ``functionregistered`` arrived (cleared by a namespace conflict).
        self.confirmed = False
        #: The connection (epoch) the last ``functionregistered`` arrived on.
        self.confirmed_epoch = -1
        #: In-process dispatch: ``functionregistered.local_dispatch`` ∧ the option. Survives a disconnect.
        self.local_dispatch = False
        #: The namespace the engine registered the function in.
        self.namespace: str | None = None
        #: The highest revision received on this connection.
        self.revision = -1
        #: The connection (epoch) on which the engine refused the function
        #: (``FUNCTION_NAMESPACE_CONFLICT``): on it, a flush counts its reports
        #: ``INACTIVE`` (also those of a call already running). A later
        #: connection may accept the replayed registration, so its reports wait
        #: for the answer; ``functionregistered`` clears it.
        self.released_epoch: int | None = None
        self._known: dict[str, Observer] = {}
        #: The active observers: one immutable tuple, swapped on change.
        self.compiled: tuple[Observer, ...] = ()
        self._lock = threading.Lock()
        self._tail: asyncio.Task[None] | None = None
        self._disposed = False
        #: Bumped when the function is released (namespace conflict, unregister):
        #: an add whose hook returns after it is undone.
        self._releases = 0

    @property
    def needs_observe(self) -> bool:
        """Callbacks or owner delivery: what needs ``observe.v1`` on both sides."""
        hooks = self.hooks
        return self.delivery == "owner" or (
            hooks is not None and (hooks.on_observer_added is not None or hooks.on_observer_removed is not None)
        )

    def is_observed(self) -> bool:
        return len(self.compiled) > 0

    def observers(self) -> list[Observer]:
        return list(self.compiled)

    # ── engine messages (SDK loop) ─────────────────────────────────────────

    def on_function_registered(
        self, namespace: str, revision: int, local_dispatch: bool, observers: list[ObserverInfoWire], epoch: int
    ) -> None:
        """``functionregistered``: always accepted; replaces the set (§4.2)."""
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
        """``RegistrationRejected{FUNCTION_NAMESPACE_CONFLICT}`` on connection
        ``epoch``: no local dispatch, no observers, and its held reports are
        released (they could never be confirmed)."""
        self.confirmed = False
        self.confirmed_epoch = -1
        self.local_dispatch = False
        self.released_epoch = epoch
        self.host.reporter.discard(self.function_id)
        self._drop_all()

    def dispose(self) -> None:
        """The function was unregistered (any thread): local dispatch stops at
        once, and ``on_observer_removed`` runs for every held observer."""
        self._disposed = True
        self.confirmed = False
        self.confirmed_epoch = -1
        self.local_dispatch = False
        self.host.reporter.discard(self.function_id)
        self.host.call_soon(self._drop_all)

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
                log.exception("Observe hook failed for %s", self.function_id)

        self._tail = asyncio.get_running_loop().create_task(run())

    # ── the active set ─────────────────────────────────────────────────────

    def _activate(self, obs: Observer) -> bool:
        """Puts the observer in the active set, unless it was rejected
        (``reject`` takes the same lock, so it cannot slip in between)."""
        with self._lock:
            if obs._rejection is not None:
                return False
            if not obs._active:
                obs._active = True
                self.compiled = (*self.compiled, obs)
            return True

    def _deactivate(self, obs: Observer) -> None:
        with self._lock:
            if not obs._active:
                return
            obs._active = False
            self.compiled = tuple(o for o in self.compiled if o is not obs)

    def _ack(self, observer_id: str, revision: int, namespace: str | None, error: dict[str, str] | None = None) -> None:
        self.host.send_ack(self.function_id, namespace, observer_id, revision, error)

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
            legacy = {"trigger_type": info.legacy.trigger_type, "config": info.legacy.config} if info.legacy else None
            if existing is not None and _same_json(existing.args, info.args) and _same_json(existing.legacy, legacy):
                self._keep(existing, info, revision, namespace)
            else:
                if existing is not None:
                    await self._remove(info.id)
                await self._add(info, revision, namespace, releases, fresh=False)

    def _keep(self, obs: Observer, info: ObserverInfoWire, revision: int, namespace: str) -> None:
        """A kept observer: the hook already ran for it; take the engine's
        filters and ack it again (with its late rejection, if it has one)."""
        obs._revision = revision
        if obs._rejection is not None:
            self._ack(info.id, revision, namespace, obs._rejection)
            return
        filter_ = obs._filter
        if not _same_json(obs.filters, info.filters):
            try:
                filter_ = compile_filter(info.filters, trusted=True)
            except Exception as error:
                self._deactivate(obs)
                self._ack(info.id, revision, namespace, {"code": "INVALID_FILTER", "message": str(error)})
                return
        obs._update(info, filter_)
        if self._activate(obs):
            self._ack(info.id, revision, namespace)
        else:
            self._ack(info.id, revision, namespace, obs._rejection)

    async def _add(self, info: ObserverInfoWire, revision: int, namespace: str, releases: int, *, fresh: bool) -> None:
        """handleAdded (§4.2): hook, then the active set, then the ack.
        ``releases`` is the release count when the message was queued: an add
        queued before the function was released never runs its hook."""
        if self._released_since(releases):
            return
        existing = self._known.get(info.id)
        if existing is not None:
            if fresh and existing._rejection is not None:
                # A fresh `observeradded` for an observer the owner rejected late: a new start.
                await self._remove(info.id)
                if self._released_since(releases):
                    return  # released while its removal hook ran
            else:
                self._keep(existing, info, revision, namespace)
                return
        try:
            filter_ = compile_filter(info.filters, trusted=True)
        except Exception as error:
            self._ack(info.id, revision, namespace, {"code": "INVALID_FILTER", "message": str(error)})
            return
        obs = Observer(self, info, filter_, revision)
        on_added = self.hooks.on_observer_added if self.hooks is not None else None
        if on_added is not None:
            obs._adding = True
            outcome = await self._run_hook(on_added, obs)
            if not outcome.ok:
                obs._adding = False
                self._ack(info.id, revision, namespace, {"code": outcome.code, "message": outcome.message})
                if outcome.late is not None:
                    # It may still finish and index the observer: undo that once it does.
                    def undo(task: asyncio.Future[Any]) -> None:
                        if task.cancelled() or task.exception() is not None:
                            return  # it failed after all: nothing was indexed
                        self._enqueue(lambda: self._call_removed(obs))

                    outcome.late.add_done_callback(undo)
                return
        released = self._released_since(releases)
        with self._lock:
            # Known before it stops being "adding": a `reject()` from another
            # thread right now finds it one way or the other.
            if not released:
                self._known[obs.id] = obs
            obs._adding = False
        if released:
            # The function was released while the hook ran: undo it, no ack.
            if on_added is not None:
                await self._call_removed(obs)
            return
        # In the active set before the ack: the first event after `active` is
        # evaluated. Refused when it was rejected (`obs.reject`, also while its
        # hook ran): the rejection wins.
        if self._activate(obs):
            self._ack(info.id, revision, namespace)
        else:
            self._ack(info.id, revision, namespace, obs._rejection)

    async def _remove(self, observer_id: str) -> None:
        obs = self._known.pop(observer_id, None)
        if obs is None:
            return
        self._deactivate(obs)
        await self._call_removed(obs)

    async def _call_removed(self, obs: Observer) -> None:
        on_removed = self.hooks.on_observer_removed if self.hooks is not None else None
        if on_removed is None:
            return
        outcome = await self._run_hook(on_removed, obs)
        if not outcome.ok:
            log.error("on_observer_removed failed for observer %s of %s: %s", obs.id, self.function_id, outcome.message)

    async def _run_hook(self, hook: ObserveHook, obs: Observer) -> _HookOutcome:
        task = asyncio.ensure_future(self.host.invoke_hook(hook, obs))
        timeout = self.host.hook_timeout_s()
        done, _ = await asyncio.wait({task}, timeout=timeout)
        if not done:
            return _HookOutcome(
                False,
                "OWNER_TIMEOUT",
                f"observe hook of {self.function_id} still running after {int(timeout * 1000)}ms",
                late=task,
            )
        if task.cancelled():
            return _HookOutcome(False, "OBSERVE_REJECTED", "the observe hook was cancelled")
        error = task.exception()
        if error is None:
            return _HookOutcome(True)
        if isinstance(error, ObserveRejection):
            return _HookOutcome(False, error.code, error.message)
        return _HookOutcome(False, "OBSERVE_REJECTED", str(error) or type(error).__name__)

    # ── owner delivery (any thread) ────────────────────────────────────────

    def _require_owner(self, what: str) -> None:
        if self.delivery != "owner":
            raise InvocationError(
                code="NOT_OWNER_DELIVERED",
                message=(
                    f"obs.{what}() needs an owner-delivered function; {self.function_id} is auto-delivered "
                    "(observe.delivery)"
                ),
                function_id=self.function_id,
            )

    def _is_active(self, obs: Observer) -> bool:
        return obs._active and self._known.get(obs.id) is obs

    def _event_caller(self, cur: ActiveInvocation | None) -> tuple[dict[str, Any] | None, bool]:
        """The caller an event carries (sdk.md §1): inside the function's own
        handler, that invocation's caller; inside another handler, that
        handler; outside any handler, this worker only."""
        if cur is not None and cur.function_id == self.function_id:
            return cur.caller, True
        caller: dict[str, Any] = dict(self.host.identity())
        if cur is not None:
            caller["function_id"] = cur.function_id
            caller["invocation_id"] = cur.invocation_id
            parent = strip_caller(cur.caller)
            if parent:
                caller["parent"] = parent
        return caller, False

    @staticmethod
    def _doc(input_: Any, caller: dict[str, Any] | None, metadata: Any) -> dict[str, Any]:
        doc: dict[str, Any] = {"input": input_}
        if caller is not None:
            doc["caller"] = caller
        if metadata is not None:
            doc["metadata"] = metadata
        return doc

    def emit(self, obs: Observer, event: Any) -> bool:
        self._require_owner("emit")
        if not self._is_active(obs):
            return False
        cur = current_context()
        caller, own = self._event_caller(cur)
        metadata = cur.metadata if cur is not None else None
        doc = self._doc(event, caller, metadata)
        if not obs._filter.matches(doc):
            return False
        traceparent, baggage = self.host.trace_headers()
        if traceparent is None and cur is not None:
            traceparent, baggage = cur.traceparent, cur.baggage
        self.host.reporter.push_emit(
            ReportInit(
                function_id=self.function_id,
                namespace=self.namespace,
                invocation_id=cur.invocation_id if own and cur is not None else str(uuid.uuid4()),
                matched=[],
                status="success",
                values=doc,
                started_at_ms=time.time() * 1000,
                duration_ms=0.0,
                observe_depth=cur.observe_depth if cur is not None else 0,
                traceparent=traceparent,
                baggage=baggage,
            ),
            obs.id,
            event,
            cur,
            obs.capture,
        )
        return True

    def check_call(self, obs: Observer, payload: Any) -> None:
        """``obs.call``: owner delivery to an active observer whose filters accept the payload."""
        self._require_owner("call")
        if not self._is_active(obs):
            raise InvocationError(
                code="OBSERVER_NOT_ACTIVE",
                message=f"observer {obs.id} of {self.function_id} is not active",
                function_id=self.function_id,
            )
        cur = current_context()
        caller, _ = self._event_caller(cur)
        if not obs._filter.matches(self._doc(payload, caller, cur.metadata if cur is not None else None)):
            raise InvocationError(
                code="FILTERS_NOT_MATCHED",
                message=f"the filters of observer {obs.id} reject this payload",
                function_id=self.function_id,
            )

    def reject(self, obs: Observer, code: str, message: str) -> None:
        with self._lock:
            if obs._adding:
                # The add acks with it once the hook returns.
                obs._rejection = {"code": code, "message": message}
                return
            if self._known.get(obs.id) is not obs:
                return
            # Kept on the observer: if this ack is lost with the connection,
            # the replayed set gets it again (`_keep`).
            obs._rejection = {"code": code, "message": message}
        self._deactivate(obs)
        self._ack(obs.id, obs._revision, self.namespace, obs._rejection)

    # ── auto delivery: a local invocation (any thread) ─────────────────────

    def report_local(
        self,
        compiled: tuple[Observer, ...],
        *,
        invocation_id: str,
        ok: bool,
        input_: Any,
        output: Any = None,
        error: dict[str, Any] | None = None,
        caller: dict[str, Any] | None = None,
        metadata: Any = None,
        started_at_ms: float,
        duration_ms: float,
        observe_depth: int,
        traceparent: str | None = None,
        baggage: str | None = None,
    ) -> None:
        """Evaluates the active set captured at call start on a finished local
        invocation of an auto function, and reports it when an observer
        matches. Never raises."""
        if self.delivery != "auto" or not compiled:
            return
        doc = self._doc(input_, caller, metadata)
        if ok:
            doc["output"] = output
        else:
            doc["error"] = error
        matched: list[str] = []
        captures: dict[str, None] = {}
        for obs in compiled:
            if not _on_accepts(obs.on, ok) or not obs._filter.matches(doc):
                continue
            matched.append(obs.id)
            for c in obs.capture:
                captures[c] = None
        if not matched:
            return
        self.host.reporter.push(
            ReportInit(
                function_id=self.function_id,
                namespace=self.namespace,
                invocation_id=invocation_id,
                matched=matched,
                status="success" if ok else "error",
                values=doc,
                started_at_ms=started_at_ms,
                duration_ms=duration_ms,
                observe_depth=observe_depth,
                traceparent=traceparent,
                baggage=baggage,
            ),
            captures,
        )
