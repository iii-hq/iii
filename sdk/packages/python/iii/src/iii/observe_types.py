"""Public types of observe (sdk.md §1, §2): the observer's source, options and
envelope, and the hooks a function registers to learn who observes it."""

from __future__ import annotations

from collections.abc import Awaitable, Callable
from typing import TYPE_CHECKING, Any, Literal, TypedDict, Union

if TYPE_CHECKING:
    from .observable import Observer

#: What an observer ships to its target. Default ``["input", "output", "caller"]``.
Capture = Literal["input", "output", "error", "caller", "metadata"]

#: An observer's lifecycle: ``pending`` (no owner yet, waiting for the owner's
#: ack, or no target yet) → ``active`` or ``error``; ``suspended`` while its
#: worker or target is away; ``disposed`` at the end.
ObserverState = Literal["pending", "active", "suspended", "error", "disposed"]

#: A declarative filter over ``input``, ``output``, ``error``, ``caller`` and
#: ``metadata``, with explicit operators only: ``{"input.amount": {"$gte": 500}}``.
Filter = dict[str, Any]

Delivery = Literal["auto", "owner"]


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
    """What the target of an auto observer (or of ``obs.emit``) receives with
    ``deliver: "envelope"`` (the default). Fields not in the observer's
    ``capture`` are absent."""

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
        args: Owner-delivered functions only: for the owner; validated
            against its ``args_format``.
        filters: Which events reach the target; ``{}`` (or absent) = every event.
        on: Auto only; default ``"success"``.
        capture: Auto and ``obs.emit``; default ``["input", "output", "caller"]``.
        deliver: Auto and ``obs.emit``; default ``"envelope"``.
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


#: ``on_observer_added`` / ``on_observer_removed``: sync or async callables.
ObserveHook = Callable[["Observer"], Union[None, Awaitable[None]]]

_UNSET: Any = object()


class ObserveHooks:
    """The ``observe`` option of ``register_function``: how the function's
    observers are delivered, and the callbacks that run when one is added or
    removed.

    Pass an instance, built with keyword arguments or from a subclass that
    sets the attributes and defines the callbacks as (sync or async) methods.
    Only ``delivery``, ``args_format``, ``legacy_trigger_type`` and ``allow``
    travel to the engine; the callbacks stay in this process.

    Attributes:
        delivery: ``"auto"`` (default): every invocation is offered to every
            observer. ``"owner"``: the handler hands each event to the
            observers it belongs to, with ``obs.emit`` / ``obs.call``.
        args_format: Owner only: JSON Schema (or a Pydantic model) of the
            observers' ``args``; the engine answers ``INVALID_ARGS``.
        legacy_trigger_type: Accept old ``register_trigger({"type": ...})``
            calls for this trigger type (the engine's legacy shim).
        allow: Default ``True``; ``False`` refuses every observer (``NOT_OBSERVABLE``).
        on_observer_added: Runs once per added observer, before the SDK acks
            it. Raise to reject it (an :class:`~iii.ObserveRejection` sets the
            code, anything else is ``OBSERVE_REJECTED``); after 30 s it is
            rejected with ``OWNER_TIMEOUT``.
        on_observer_removed: Runs once per removed observer. Cannot fail:
            errors are logged.

    The callbacks of one function run one at a time, in revision order, on
    the SDK's event loop (a sync one on a thread of its own, so it may call
    the sync API). A callback still running after the 30 s timeout no longer
    holds the others back: from then on it may overlap with the next ones.

    Examples:
        >>> class CronHooks(ObserveHooks):
        ...     delivery = "owner"
        ...     args_format = {"type": "object", "required": ["expression"],
        ...                    "properties": {"expression": {"type": "string"}}}
        ...
        ...     async def on_observer_added(self, obs):
        ...         scheduler.retain(obs.args["expression"], obs)
        ...
        ...     async def on_observer_removed(self, obs):
        ...         scheduler.release(obs.id)
        >>> iii.register_function("scheduler::on-tick", on_tick, observe=CronHooks())
        >>> iii.register_function("orders::create", create, observe=ObserveHooks(
        ...     on_observer_added=lambda obs: print("observed by", obs.target)))
    """

    delivery: Delivery | None = None
    args_format: Any = None
    legacy_trigger_type: str | None = None
    allow: bool | None = None
    on_observer_added: ObserveHook | None = None
    on_observer_removed: ObserveHook | None = None

    def __init__(
        self,
        *,
        delivery: Delivery | None = _UNSET,
        args_format: Any = _UNSET,
        legacy_trigger_type: str | None = _UNSET,
        allow: bool | None = _UNSET,
        on_observer_added: ObserveHook | None = _UNSET,
        on_observer_removed: ObserveHook | None = _UNSET,
    ) -> None:
        # Keyword arguments override the (sub)class attributes; unset ones keep them.
        values = {
            "delivery": delivery,
            "args_format": args_format,
            "legacy_trigger_type": legacy_trigger_type,
            "allow": allow,
            "on_observer_added": on_observer_added,
            "on_observer_removed": on_observer_removed,
        }
        for name, value in values.items():
            if value is not _UNSET:
                setattr(self, name, value)

    @classmethod
    def from_value(cls, value: ObserveHooks | dict[str, Any] | None) -> ObserveHooks | None:
        """An ``ObserveHooks`` from what ``register_function(observe=...)`` was given."""
        if value is None or isinstance(value, ObserveHooks):
            return value
        if isinstance(value, dict):
            return cls(**value)
        raise TypeError(f"observe must be an ObserveHooks or a dict, got {type(value).__name__}")

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}(delivery={self.delivery!r}, legacy_trigger_type={self.legacy_trigger_type!r}, "
            f"allow={self.allow!r})"
        )
