"""Public types of observe: the observer's source, options and envelope, the
:class:`Call` a function's observers observe, and the hooks the worker that
hosts a function sets to decide what observing it means."""

from __future__ import annotations

from collections.abc import Awaitable, Callable, Iterable, Sequence
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any, Generic, Literal, TypedDict, TypeVar, Union

if TYPE_CHECKING:
    from .observable import Observer

#: What an observer ships to its target. Default ``["input", "output", "caller"]``.
Capture = Literal["input", "output", "error", "caller", "metadata"]

#: An observer's lifecycle: ``pending`` (no host yet, waiting for the host's
#: ack, or no target yet) → ``active`` or ``error``; ``suspended`` while its
#: worker or target is away; ``disposed`` at the end.
ObserverState = Literal["pending", "active", "suspended", "error", "disposed"]

#: A declarative filter over ``input``, ``output``, ``error``, ``caller`` and
#: ``metadata``, with explicit operators only: ``{"input.amount": {"$gte": 500}}``.
Filter = dict[str, Any]


class TargetRef(TypedDict, total=False):
    function_id: str
    namespace: str


#: The right side of ``iii.observe``: a registered function id, or ``{function_id, namespace?}``.
Target = Union[str, TargetRef]


class ObserveReason(TypedDict):
    """A state reason or an error: a machine-readable code and a message."""

    code: str
    message: str


class CallerParent(TypedDict, total=False):
    worker_id: str
    worker_name: str
    function_id: str
    invocation_id: str


class Caller(CallerParent, total=False):
    """Who started an invocation: the worker (``worker_id`` / ``worker_name``,
    stamped by the engine), and the function and invocation the call was made
    from (absent for top-level calls). ``parent`` is that handler's own caller,
    one level only."""

    parent: CallerParent


class Observed(TypedDict, total=False):
    """What an observer's target receives with ``deliver: "envelope"`` (the
    default). Fields not in the observer's ``capture`` are absent."""

    observer_id: str
    activation_id: str
    function_id: str
    namespace: str
    invocation_id: str
    status: Literal["success", "error"]
    input: Any
    output: Any
    error: dict[str, Any]
    caller: Caller
    metadata: dict[str, Any]
    started_at_ms: int
    duration_ms: float


class _ObserveSourceRequired(TypedDict):
    function_id: str


class ObserveSource(_ObserveSourceRequired, total=False):
    """The left side of ``iii.observe``: what is observed.

    Attributes:
        function_id: The observed function.
        namespace: Defaults to this worker's (``engine::*`` resolves to ``default``).
        args: For the worker that hosts the function, which reads them in its
            hooks; validated against the function's ``args_format``. A function
            without ``args_format`` takes none.
        filters: Which calls reach the target; ``{}`` (or absent) = every call.
        on: ``"success"`` (default), ``"error"`` or ``"complete"``.
        capture: Default ``["input", "output", "caller"]``.
        deliver: What the target receives when iii delivers: ``"envelope"``
            (default), ``"input"`` or ``"output"``.
    """

    namespace: str
    args: dict[str, Any]
    filters: Filter
    on: Literal["success", "error", "complete"]
    capture: list[Capture]
    deliver: Literal["envelope", "input", "output"]


class ObserverOptions(TypedDict, total=False):
    """The observer itself: its id, policy and metadata (the third argument of ``iii.observe``).

    Attributes:
        id: Default: a random UUID, stable across replays.
        max_fires: Disposed after this many activations.
        expires_in_ms: Disposed after this long.
        max_in_flight: Activations running at once (the engine queues the rest).
        metadata: Delivered as the metadata sidecar of every activation.
    """

    id: str
    max_fires: int
    expires_in_ms: int
    max_in_flight: int
    metadata: dict[str, Any]


InputT = TypeVar("InputT")
OutputT = TypeVar("OutputT")


@dataclass
class Call(Generic[InputT, OutputT]):
    """One call of a function, as its observers observe it and its
    ``on_observer_match`` and ``on_observer_deliver`` hooks receive it.

    Attributes:
        function_id: The called function.
        invocation_id: This call's id.
        status: ``"success"`` or ``"error"``.
        input: What the function was called with, as the caller passed it
            (channel refs as refs, not readers and writers).
        output: The handler's return value: what observers observe (``None``
            when it raised).
        error: What the handler raised, ``{code, message, stacktrace?}``
            (``None`` when it returned).
        caller: Who made the call: ``{worker_id?, worker_name?, function_id?,
            invocation_id?, parent?}``; ``None`` outside any handler.
        metadata: The call's metadata sidecar.
    """

    function_id: str
    invocation_id: str
    status: Literal["success", "error"]
    input: InputT
    output: OutputT | None = None
    error: dict[str, Any] | None = None
    caller: dict[str, Any] | None = None
    metadata: Any = None


#: ``on_observer_added``: runs before the observer is accepted; raise to reject it.
AddedHook = Callable[["Observer"], Union[None, Awaitable[None]]]

#: ``on_observer_removed``: runs once for every accepted observer when it goes.
RemovedHook = Callable[["Observer"], Union[None, Awaitable[None]]]

#: A match key, or several: an observer wants a call when they share one.
Key = Union[str, Iterable[str]]

#: ``on_observer_match`` as a predicate: whether this observer wants this
#: call. Asked once per active observer per call; a raise counts as no match.
MatchPredicate = Callable[["Observer", Call[Any, Any]], bool]


@dataclass(frozen=True)
class MatchKeys:
    """``on_observer_match`` as keys: an observer wants a call when they share
    a key, and the SDK keeps an index instead of asking every observer.

    Attributes:
        observer: What an observer listens for, computed once when it is
            accepted; an observer whose keys raise is rejected
            (``OBSERVE_REJECTED``).
        call: What a call is, computed once per call; a raise matches no
            observer.
    """

    observer: Callable[[Observer], Key]
    call: Callable[[Call[Any, Any]], Key]


#: ``on_observer_match``: which observers want each call of a function.
MatchHook = Union[MatchPredicate, MatchKeys]

#: ``on_observer_deliver(call, observers)``: delivers the call itself and
#: returns what the caller gets back. ``observers`` are the observers that
#: matched it, from the active set when the call started.
DeliverHook = Callable[[Call[Any, Any], Sequence["Observer"]], Any]
