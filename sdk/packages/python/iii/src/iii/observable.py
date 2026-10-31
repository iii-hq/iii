"""The host side of observe: per-function observer state, the observer hooks
(``on_observer_added`` / ``on_observer_removed`` at both levels), the
:class:`Observer` handed to them, matching (``on_observer_match``, the key
index) and each call's delivery (iii's, or ``on_observer_deliver``).

Where filters run (decision 55): the engine evaluates the observers' ``on``
and filters for every call it routes. For a routed call this host runs only
worker code, the function's match and deliver hooks, and its answer says
which observers the match hook matched (``matched``) or that the deliver
hook delivered the call (``delivered``). The engine never sees this
worker's calls of its own functions: for those, iii's delivery here applies
``on`` and filters through an index of the filters' equality conditions
(:class:`~iii.filter._index.FilterIndex`), and reports the observers
delivered.

Threading: the engine's messages are handled on the SDK loop, and the hooks
of one function id run there one at a time, in the order the engine announced
the observers (sync hooks on a thread of their own). ``Observer.matches`` /
``reject`` / ``accepted``, ``observers()`` and ``is_observed()`` may be called
from any thread: the active observers, the match hook and the key index are
one immutable :class:`ActiveSet`, swapped under a lock, and a call reads it
once, when it starts.
"""

from __future__ import annotations

import asyncio
import copy
import inspect
import json
import logging
import threading
from collections.abc import Awaitable, Callable, Coroutine, Iterable, Mapping, Sequence
from dataclasses import dataclass
from functools import cached_property
from typing import Any, Literal, Protocol, Union, cast

from .errors import ObserveRejection
from .filter import CompiledFilter, compile_filter
from .filter._index import FilterIndex
from .iii_types import ObserverInfoWire
from .invocation_context import run_in_thread
from .observe_types import AddedHook, Call, DeliverHook, MatchHook, MatchKeys, MatchPredicate, RemovedHook
from .observe_wire import report_caller
from .reporter import Reporter, ReportInit

log = logging.getLogger("iii.observe")

#: An ``on_observer_added`` (or ``on_observer_removed``) still running after this is ``OWNER_TIMEOUT``.
OBSERVE_HOOK_TIMEOUT_S = 30.0

#: The two levels a hook is set at: every function this worker hosts, or one function.
Level = Literal["worker", "function"]

#: A partial call (``{"input": ...}``) or a :class:`Call`, for ``matches`` and ``is_observed``.
CallLike = Union[Call[Any, Any], Mapping[str, Any]]

#: iii's delivery of a local call evaluates the filters of up to this many
#: observers one by one; past it, it matches them through the active set's
#: filter index, built once per set.
SCAN_MAX = 8

#: The ``on`` values that accept a call that succeeded, and one that failed.
_ON_SUCCESS = frozenset({"success", "complete"})
_ON_ERROR = frozenset({"error", "complete"})

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

    def match_hook(self, function_id: str) -> MatchHook | None:
        """The ``on_observer_match`` set for ``function_id``."""

    def hook_queue(self, function_id: str) -> HookQueue:
        """The queue the observer hooks of ``function_id`` run in, shared by
        every registration of the id."""

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


def same_data(a: Any, b: Any) -> bool:
    """Whether two copies of a value hold the same JSON data: ``None``,
    ``bool``, ``int``, ``float``, ``str``, lists and dicts with string keys,
    compared member by member in one walk (``True`` and ``1`` differ, as they
    do in JSON). Any other object is the same only as itself, and so is a
    list or dict the walk reaches twice (a shared member or a cycle). A local
    call compares the handler's copy of its input with the copy its delivery
    took when the call started."""
    left: list[Any] = [a]
    right: list[Any] = [b]
    seen: set[int] = set()
    while left:
        x = left.pop()
        y = right.pop()
        if x is y:
            continue
        if type(x) is not type(y):
            return False
        if isinstance(x, (str, int, float)) or x is None:
            if x != y and not (isinstance(x, float) and x != x and y != y):
                return False
            continue
        if not isinstance(x, (list, dict)) or id(x) in seen:
            return False
        seen.add(id(x))
        if isinstance(x, list):
            if len(x) != len(y):
                return False
            left.extend(x)
            right.extend(y)
            continue
        if len(x) != len(y):
            return False
        for key, value in x.items():
            if not isinstance(key, str) or key not in y:
                return False
            left.append(value)
            right.append(y[key])
    return True


def _same_view(obs: Observer, info: ObserverInfoWire) -> bool:
    """Whether ``obs`` is served with the view ``info`` announces: its
    filters, target, capture and ``on``."""
    return (
        obs.target == info.target
        and obs.on == info.on
        and obs.capture == list(info.capture)
        and _same_json(obs.filters, info.filters)
    )


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


def match_keys(keys: Any, what: str) -> tuple[str, ...]:
    """What a key function of ``on_observer_match`` returned, as distinct
    keys in order. Raises ``TypeError`` for anything but a string or strings."""
    if isinstance(keys, str):
        return (keys,)
    if isinstance(keys, (bytes, bytearray, Mapping)) or not isinstance(keys, Iterable):
        raise TypeError(f"on_observer_match: the {what} key function must return a string or strings")
    out = tuple(dict.fromkeys(keys))
    for key in out:
        if not isinstance(key, str):
            raise TypeError(f"on_observer_match: the {what} key function returned a {type(key).__name__} key")
    return out


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
        filters: The observer's filters, normalized; iii's delivery and
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
        #: The keys it is filed under on a function matched by keys.
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
        iii's delivery checks, for a deliver hook that calls targets itself.

        ``call`` is a :class:`Call` or a partial one (``{"input": ...}``);
        ``on`` is checked when it has a ``status``.
        """
        status = call.get("status") if isinstance(call, Mapping) else call.status
        if status is not None and not _on_accepts(self.on, status == "success"):
            return False
        return self._filter.matches(call_doc(call))

    def accepted(self, timeout: float | None = None) -> None:
        """Blocks until the SDK has acked this observer: a call made
        afterwards reaches an active observer. Use it to send a new observer
        its initial state, outside its ``on_observer_added`` (which runs
        before the ack): a call of the function with ``"metadata":
        {"for_observer": obs.id}``, which the function's match hook gives to
        this observer alone, delivered like every call. Use
        :meth:`accepted_async` in async code.

        Example::

            async def send_state(obs):
                try:
                    await obs.accepted_async()
                except ObserveRejection:
                    return  # a rejected observer gets nothing
                await iii.call_async({
                    "function_id": "tmux::on-changed",
                    "payload": state_for(obs.args),
                    "metadata": {"for_observer": obs.id},
                    "action": CallAction.Void(),
                })

            @iii.on_observer_added("tmux::on-changed")
            async def on_added(obs):
                asyncio.get_running_loop().create_task(send_state(obs))

            def wants(obs, call):
                for_observer = (call.metadata or {}).get("for_observer")
                if for_observer:
                    return for_observer == obs.id
                return obs.args["session"] == call.input["session"]

            iii.on_observer_match("tmux::on-changed", wants)

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

        The observer leaves the active set at once, and its
        ``on_observer_removed`` hooks run next on the function's hook queue,
        connected or not; the engine's ``observerremoved`` that follows runs
        none again. The rejection is sent again when the engine lists the
        observer after a reconnect. Called while its ``on_observer_added``
        runs, the rejection wins once the hook returns, and the
        ``on_observer_removed`` of each level whose hook resolved runs.
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


@dataclass(frozen=True, eq=False)
class ActiveSet:
    """A function's active observers and how a call is matched to them, as
    one immutable snapshot: a call reads it once, when it starts, and every
    change swaps in a new one (copy-on-write)."""

    observers: tuple[Observer, ...] = ()
    #: The ``on_observer_match`` this snapshot matches with.
    match: MatchHook | None = None
    #: Matching by keys: key → the observers filed under it.
    index: Mapping[str, tuple[Observer, ...]] | None = None

    @cached_property
    def filters(self) -> FilterIndex[Observer]:
        """The observers' filters, indexed by their equality conditions, in
        the order of :attr:`observers`. Built on the first read (the first
        local call that matches through it), then kept: an observer whose
        filters change goes into a new set (:meth:`renewed`)."""
        return FilterIndex((obs, obs._filter) for obs in self.observers)

    def renewed(self) -> ActiveSet:
        """The same set as a new snapshot, for observers whose filters
        changed: its filter index is built from their current filters."""
        return ActiveSet(self.observers, self.match, self.index)

    def adding(self, obs: Observer) -> ActiveSet:
        """This set with ``obs``, filed under its keys."""
        index = self.index
        if index is not None and obs._keys:
            filed = dict(index)
            for key in obs._keys:
                filed[key] = (*filed.get(key, ()), obs)
            index = filed
        return ActiveSet((*self.observers, obs), self.match, index)

    def removing(self, obs: Observer) -> ActiveSet:
        """This set without ``obs``, unfiled from its keys."""
        index = self.index
        if index is not None and obs._keys:
            filed = dict(index)
            for key in obs._keys:
                rest = tuple(o for o in filed.get(key, ()) if o is not obs)
                if rest:
                    filed[key] = rest
                else:
                    filed.pop(key, None)
            index = filed
        return ActiveSet(tuple(o for o in self.observers if o is not obs), self.match, index)

    def refiling(self, obs: Observer, keys: tuple[str, ...]) -> ActiveSet:
        """This set with ``obs`` filed under ``keys`` instead of its own
        (``obs._keys``), in the same place among the observers."""
        if self.index is None:
            return self
        filed = dict(self.index)
        for key in obs._keys:
            rest = tuple(o for o in filed.get(key, ()) if o is not obs)
            if rest:
                filed[key] = rest
            else:
                filed.pop(key, None)
        for key in keys:
            filed[key] = (*filed.get(key, ()), obs)
        return ActiveSet(self.observers, self.match, filed)


class HookQueue:
    """The observer hooks of one function id, run one at a time in the order
    they were queued, on the SDK loop. Every registration of the id shares it,
    so the hooks of a new registration run after the removals of the one
    before."""

    def __init__(self, function_id: str) -> None:
        self.function_id = function_id
        #: The last task queued.
        self.tail: asyncio.Task[None] | None = None

    def enqueue(self, task: Callable[[], Awaitable[None]]) -> None:
        """Queues ``task`` behind the last one (SDK loop)."""
        previous = self.tail

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

        self.tail = asyncio.get_running_loop().create_task(run())


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
        match = host.match_hook(function_id)
        #: The active observers, the match hook and the key index: swapped whole under ``_lock``.
        self.active = ActiveSet(match=match, index={} if isinstance(match, MatchKeys) else None)
        self._lock = threading.Lock()
        self._queue = host.hook_queue(function_id)
        self._disposed = False
        #: Bumped when the function is released (rejection, unregister): an
        #: add whose hook returns after it is undone.
        self._releases = 0
        #: A match hook raised: logged once per function.
        self._match_failure_logged = False

    def is_observed(self, call: CallLike | None = None) -> bool:
        """Any active observer; with a call on a function matched by keys,
        any observer filed under one of the call's keys."""
        active = self.active
        if not active.observers:
            return False
        if call is None or active.index is None:
            return True
        index = active.index
        keys = self._call_keys(cast(MatchKeys, active.match), as_call(self.function_id, call))
        return any(key in index for key in keys)

    def observers(self) -> list[Observer]:
        return list(self.active.observers)

    # ── matching (any thread) ──────────────────────────────────────────────

    def matching(self, active: ActiveSet, call: Call[Any, Any]) -> tuple[Observer, ...]:
        """The observers of ``active`` that want ``call``: all of them
        without ``on_observer_match``, those the predicate accepts, or those
        filed under one of the call's keys, each once. A match hook that
        raises matches nothing, and is logged once per function."""
        match = active.match
        if match is None or not active.observers:
            return active.observers
        if active.index is not None:
            index = active.index
            found: dict[str, Observer] = {}
            for key in self._call_keys(cast(MatchKeys, match), call):
                for obs in index.get(key, ()):
                    found.setdefault(obs.id, obs)
            return tuple(found.values())
        predicate = cast(MatchPredicate, match)
        chosen: list[Observer] = []
        for obs in active.observers:
            try:
                if predicate(obs, call):
                    chosen.append(obs)
            except Exception as error:
                self._match_failed(error)
        return tuple(chosen)

    def _call_keys(self, match: MatchKeys, call: Call[Any, Any]) -> tuple[str, ...]:
        """The keys of a call; a key function that raises gives none."""
        try:
            return match_keys(match.call(call), "call")
        except Exception as error:
            self._match_failed(error)
            return ()

    def _match_failed(self, error: BaseException) -> None:
        if self._match_failure_logged:
            return
        self._match_failure_logged = True
        log.error("on_observer_match of %s raised; the call matches no observer", self.function_id, exc_info=error)

    def _keys_failure(self, obs: Observer, error: BaseException) -> str:
        return f"the keys of observer {obs.id} of {self.function_id} could not be computed: {error}"

    def match_changed(self) -> None:
        """The function's ``on_observer_match`` was set or removed (any
        thread): the active set takes it on the SDK loop."""
        self.host.call_soon(self._rebuild)

    def _rebuild(self) -> None:
        """Takes the function's current ``on_observer_match`` (SDK loop).
        Matching by keys files the active observers anew; their keys are
        computed outside the lock, and an observer whose keys raise is
        rejected (``OBSERVE_REJECTED``)."""
        match = self.host.match_hook(self.function_id)
        if self._disposed or match is self.active.match:
            return
        computed: list[tuple[Observer, tuple[str, ...]]] = []
        failed: list[tuple[Observer, BaseException]] = []
        if isinstance(match, MatchKeys):
            for obs in self.active.observers:
                try:
                    computed.append((obs, match_keys(match.observer(obs), "observer")))
                except Exception as error:
                    failed.append((obs, error))
        with self._lock:
            # Observers are only added on the SDK loop, so none was added
            # meanwhile; a `reject()` from another thread may have removed some.
            observers = self.active.observers
            still = {id(obs) for obs in observers}
            for obs in observers:
                obs._keys = ()
            index: dict[str, tuple[Observer, ...]] | None = None
            if isinstance(match, MatchKeys):
                filed: dict[str, list[Observer]] = {}
                for obs, keys in computed:
                    if id(obs) not in still:
                        continue
                    obs._keys = keys
                    for key in keys:
                        filed.setdefault(key, []).append(obs)
                index = {key: tuple(found) for key, found in filed.items()}
            self.active = ActiveSet(observers, match, index)
        for obs, failure in failed:
            obs.reject("OBSERVE_REJECTED", self._keys_failure(obs, failure))

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
        if self.released_epoch is not None:
            # Refused or withdrawn: the function is not this worker's. No hook runs, no ack goes out.
            log.debug("observeradded for released function %s ignored", self.function_id)
            return
        self.revision = revision
        releases = self._releases

        async def added() -> None:
            # Released since it arrived: nothing to add, nothing to ack.
            if self._released_since(releases):
                return
            held = self._known.get(info.id)
            if held is not None:
                if self._reack(held, info, revision, namespace, listed=False):
                    return
                # Other args, or rejected late: a fresh observer, after the one held is removed.
                await self._remove(info.id)
            await self._add(info, revision, namespace, releases)

        self._enqueue(added)

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
            active = self.active
            for obs in active.observers:
                obs._active = False
            self.active = ActiveSet(match=active.match, index=None if active.index is None else {})

        async def remove_all() -> None:
            for observer_id in list(self._known):
                await self._remove(observer_id)

        self._enqueue(remove_all)

    def _enqueue(self, task: Callable[[], Awaitable[None]]) -> None:
        self._queue.enqueue(task)

    # ── the active set ─────────────────────────────────────────────────────

    def _activate(self, obs: Observer) -> bool:
        """Puts the observer in the active set, filed under its keys when the
        function is matched by keys, unless it was rejected (``reject`` takes
        the same lock, so it cannot slip in between). Its keys are computed
        outside the lock, since a key function may call ``obs.reject()``; an
        observer whose keys raise is rejected (``OBSERVE_REJECTED``)."""
        while True:
            if obs._active:
                return True
            match = self.active.match
            keys: tuple[str, ...] = ()
            if isinstance(match, MatchKeys):
                try:
                    keys = match_keys(match.observer(obs), "observer")
                except Exception as error:
                    with self._lock:
                        if obs._rejection is None:
                            obs._rejection = {"code": "OBSERVE_REJECTED", "message": self._keys_failure(obs, error)}
                    return False
            with self._lock:
                if obs._rejection is not None:
                    return False
                if obs._active:
                    return True
                if self.active.match is not match:
                    continue  # the match hook changed while the keys were computed
                obs._keys = keys
                obs._active = True
                obs._accepted = True
                self.active = self.active.adding(obs)
                return True

    def _deactivate(self, obs: Observer) -> None:
        with self._lock:
            if not obs._active:
                return
            obs._active = False
            self.active = self.active.removing(obs)

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
            held = self._known.get(info.id)
            if held is not None:
                if self._reack(held, info, revision, namespace, listed=True):
                    continue
                # Other args: removed, then added as a new observer.
                await self._remove(info.id)
            await self._add(info, revision, namespace, releases)

    def _reack(self, obs: Observer, info: ObserverInfoWire, revision: int, namespace: str, *, listed: bool) -> bool:
        """Acks again, at ``revision`` and without running any hook, an
        observer this SDK holds that the engine announced again (decision 44):
        one with the same args, served from now on with the filters, target,
        capture and ``on`` it was announced with (filed under its keys again
        when they changed), or, when ``listed`` by ``functionregistered``, one
        rejected late, whose rejection is sent again. ``False`` for an
        observer to add as a new one after removing the one held: other args, a
        late rejection announced by ``observeradded``, or a view this SDK
        cannot serve (filters that do not compile, keys that raise)."""
        changed = _same_json(obs.args, info.args) and not _same_view(obs, info)
        # Prepared outside the lock: key functions are the worker's code.
        view = self._prepare_view(obs, info) if changed else None
        with self._lock:
            # Under the lock, ordered with `reject()` from another thread.
            rejection = obs._rejection
            if rejection is not None:
                if not listed:
                    return False
            elif not _same_json(obs.args, info.args):
                return False
            elif changed:
                if view is None:
                    return False
                filter_, keys, match = view
                active = self.active
                if keys is not None and obs._active and active.match is match:
                    active = active.refiling(obs, keys)
                    obs._keys = keys
                if obs._active and filter_ is not obs._filter and active is self.active:
                    active = active.renewed()
                # The observer takes its new filters before the set that
                # serves them is published, so the filter index a call on
                # any thread builds for that set files these filters.
                obs._update(info, filter_)
                self.active = active
            obs._revision = revision
        self._ack(obs, revision, namespace, rejection)
        return True

    def _prepare_view(
        self, obs: Observer, info: ObserverInfoWire
    ) -> tuple[CompiledFilter, tuple[str, ...] | None, MatchHook | None] | None:
        """What ``obs`` is served with once it takes the view ``info``: its
        compiled filters, and its keys on a function matched by keys (computed
        on a copy, so ``obs`` stays as it was), with the match hook they are
        for. ``None`` for a view this SDK cannot serve."""
        filter_ = obs._filter
        if not _same_json(obs.filters, info.filters):
            try:
                filter_ = compile_filter(info.filters, trusted=True)
            except Exception:
                return None
        match = self.active.match
        if not isinstance(match, MatchKeys):
            return filter_, None, match
        view = copy.copy(obs)
        view._update(info, filter_)
        try:
            return filter_, match_keys(match.observer(view), "observer"), match
        except Exception:
            return None

    async def _add(self, info: ObserverInfoWire, revision: int, namespace: str, releases: int) -> None:
        """An added observer: the worker's ``on_observer_added``, then the
        function's, then the active set, then the ack. A hook that raises (or
        runs past the timeout) rejects the observer, and the
        ``on_observer_removed`` of each level whose hook already resolved
        runs. ``releases`` is the release count when the message was queued:
        an add queued before the function was released never runs its hooks."""
        if self._released_since(releases):
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
            if self._released_since(releases):
                break  # no further hook runs for a released function
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
        behind = [obs for obs in self.active.observers if level not in obs._setup]
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
            # the replayed set gets it again (`_reack`).
            obs._rejection = {"code": code, "message": message}
        self._deactivate(obs)
        self._ack(obs, obs._revision, self.namespace, obs._rejection)
        # The engine sends no `observerremoved` for a rejection that answers an
        # announcement it has not seen acked (a replay after a reconnect): the
        # removed hooks run now, once, and the observer stays known with its
        # rejection.
        self.host.call_soon(lambda: self._enqueue(lambda: self._tear_down(obs)))

    async def _tear_down(self, obs: Observer) -> None:
        """Runs the ``on_observer_removed`` hooks of an observer rejected late,
        the function's then the worker's, once. It stays known (inactive)
        until the engine removes it or announces it afresh, and that removal
        runs no hook again."""
        if self._known.get(obs.id) is not obs or not obs._accepted or obs._active:
            return
        obs._accepted = False
        obs._setup.clear()
        await self._run_removed(obs, ("function", "worker"))


class Delivery:
    """One call's delivery: the observers that want it (``on_observer_match``,
    from the active set when the call started), then the function's
    ``on_observer_deliver``, which delivers the call itself, or iii's
    delivery. For a local call, iii's delivery reports the matched observers
    whose ``on`` and filters match the call in one report item. For a routed
    call the engine evaluates ``on`` and filters: the answer lists in
    ``matched`` the observers the function's match hook matched (``[]`` when
    none), and carries no list for a function without one (decision 55)."""

    def __init__(
        self,
        owner: FunctionObservers,
        call: Call[Any, Any],
        handler_error: BaseException | None,
        active: ActiveSet,
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
        self._active = active
        self._routed = routed
        self._started_at_ms = started_at_ms
        self._duration_ms = duration_ms
        self._observe_depth = observe_depth
        #: A routed call's ``matched``: the observers its match hook matched,
        #: before ``on`` and filters. ``None`` for a function without one.
        self.matched: list[str] | None = None

    async def run_async(self, hook: DeliverHook | None) -> tuple[bool, Any]:
        """Delivers the call and returns ``(ok, value)``: what the caller gets
        back, or the error it gets. A sync hook runs on a thread of its own."""
        matched = self.owner.matching(self._active, self.call)
        if hook is None:
            return self._deliver(matched)
        try:
            if is_async_callable(hook):
                result = await hook(self.call, matched)
            else:
                result = await run_in_thread(hook, self.call, matched)
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
        run_coroutine: Callable[[Coroutine[Any, Any, Any]], Any],
    ) -> tuple[bool, Any]:
        """:meth:`run_async` on the calling thread: a sync hook runs here, an
        async one through ``run_coroutine`` (on the SDK loop)."""
        matched = self.owner.matching(self._active, self.call)
        if hook is None:
            return self._deliver(matched)
        try:
            if is_async_callable(hook):
                result = run_coroutine(hook(self.call, matched))
            else:
                result = hook(self.call, matched)
                if inspect.iscoroutine(result):
                    result = run_coroutine(result)
            return True, result
        except BaseException as error:
            return False, error

    def _deliver(self, matched: Sequence[Observer]) -> tuple[bool, Any]:
        """iii's delivery: a routed call's answer names the observers the
        match hook matched, and a local call is reported to the matched
        observers whose ``on`` and filters match it. Returns the handler's
        outcome."""
        call = self.call
        ok = call.status == "success"
        if self._routed:
            if self._active.match is not None:
                self.matched = [obs.id for obs in matched]
        else:
            selected = self._select(matched, ok)
            if selected:
                self.owner.host.reporter.push(*self._item(selected))
        if ok:
            return True, call.output
        return False, self.handler_error

    def _select(self, matched: Sequence[Observer], ok: bool) -> list[Observer]:
        """The matched observers whose ``on`` and filters match the call, in
        order. Past :data:`SCAN_MAX` observers, through the active set's
        filter index: every observer when there is no match hook, or the
        match hook's choice, each looked up before its filter runs."""
        if not matched:
            return []
        doc = call_doc(self.call)
        accepted = _ON_SUCCESS if ok else _ON_ERROR  # what `_on_accepts` accepts
        if len(matched) <= SCAN_MAX:
            return [obs for obs in matched if obs.on in accepted and obs._filter.matches(doc)]
        active = self._active
        if matched is active.observers:
            return [
                entry.key
                for entry in active.filters.candidate_entries(doc)
                if entry.key.on in accepted and entry.filter.matches(doc)
            ]
        return active.filters.select(matched, doc, lambda obs: obs.on in accepted)

    def _item(self, selected: list[Observer]) -> tuple[ReportInit, set[str]]:
        """The report item of a local call delivered to ``selected``."""
        call = self.call
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
        for obs in selected:
            captures.update(obs.capture)
        traceparent, baggage = self.owner.host.trace_headers()
        init = ReportInit(
            function_id=self.owner.function_id,
            namespace=self.owner.namespace,
            invocation_id=call.invocation_id,
            matched=[obs.id for obs in selected],
            status="success" if call.status == "success" else "error",
            values=values,
            started_at_ms=self._started_at_ms,
            duration_ms=self._duration_ms,
            observe_depth=self._observe_depth,
            traceparent=traceparent,
            baggage=baggage,
            output_is_input=(self.owner.host.delivers_v2() and call.status == "success" and call.output is call.input),
        )
        return init, captures
