"""The handle ``register_function`` returns."""

from __future__ import annotations

from collections.abc import Callable
from typing import TYPE_CHECKING, Any, TypeVar, overload

if TYPE_CHECKING:
    from .iii import III
    from .observable import CallLike, Observer
    from .observe_types import AddedHook, Call, DeliverHook, Key, MatchKeys, MatchPredicate, RemovedHook

AddedT = TypeVar("AddedT", bound="AddedHook")
RemovedT = TypeVar("RemovedT", bound="RemovedHook")
MatchT = TypeVar("MatchT", bound="MatchPredicate")
DeliverT = TypeVar("DeliverT", bound="DeliverHook")

#: The hook of ``on_observer_match`` that was not passed (the keys form).
_MISSING: Any = object()


class FunctionRef:
    """A registered function: the client's hook methods with its id filled in,
    its observers, and ``unregister()``.

    Each hook method works as a decorator and as a plain call, returns the
    hook, and removes the hook when given ``None``. Hooks are stored by
    function id, so they stay in place across ``unregister()`` and a later
    registration of the same id.

    Attributes:
        id: The function's id.

    Examples:
        >>> on_tick = iii.register_function("cron::on-tick", lambda tick: tick, private=True)
        >>> @on_tick.on_observer_added
        ... def start(obs):
        ...     scheduler.retain(obs.args["expression"])
        >>> on_tick.on_observer_match(lambda obs, call: obs.args["expression"] == call.input["expression"])
    """

    def __init__(self, client: III, function_id: str, unregister: Callable[[], None]) -> None:
        self._client = client
        self.id = function_id
        self._unregister = unregister

    @overload
    def on_observer_added(self, hook: AddedT) -> AddedT: ...
    @overload
    def on_observer_added(self, hook: None) -> None: ...
    def on_observer_added(self, hook: AddedHook | None) -> AddedHook | None:
        """This function's ``on_observer_added`` (see ``III.on_observer_added``)."""
        return self._client.on_observer_added(self.id, hook)

    @overload
    def on_observer_removed(self, hook: RemovedT) -> RemovedT: ...
    @overload
    def on_observer_removed(self, hook: None) -> None: ...
    def on_observer_removed(self, hook: RemovedHook | None) -> RemovedHook | None:
        """This function's ``on_observer_removed`` (see ``III.on_observer_removed``)."""
        return self._client.on_observer_removed(self.id, hook)

    @overload
    def on_observer_match(self, hook: MatchT) -> MatchT: ...
    @overload
    def on_observer_match(self, hook: MatchKeys) -> MatchKeys: ...
    @overload
    def on_observer_match(self, hook: None) -> None: ...
    @overload
    def on_observer_match(
        self, *, observer: Callable[[Observer], Key], call: Callable[[Call[Any, Any]], Key]
    ) -> MatchKeys: ...
    def on_observer_match(self, hook: Any = _MISSING, *, observer: Any = None, call: Any = None) -> Any:
        """This function's ``on_observer_match``: a predicate ``(obs, call) ->
        bool``, or ``observer=`` and ``call=`` key functions (see
        ``III.on_observer_match``)."""
        if hook is _MISSING:
            return self._client.on_observer_match(self.id, observer=observer, call=call)
        if observer is not None or call is not None:
            raise TypeError("on_observer_match takes a predicate or observer= and call= key functions, not both")
        return self._client.on_observer_match(self.id, hook)

    @overload
    def on_observer_deliver(self, hook: DeliverT) -> DeliverT: ...
    @overload
    def on_observer_deliver(self, hook: None) -> None: ...
    def on_observer_deliver(self, hook: DeliverHook | None) -> DeliverHook | None:
        """This function's ``on_observer_deliver`` (see ``III.on_observer_deliver``)."""
        return self._client.on_observer_deliver(self.id, hook)

    def observers(self) -> list[Observer]:
        """The function's active observers, as this worker sees them."""
        return self._client.observers(self.id)

    def is_observed(self, call: CallLike | None = None) -> bool:
        """Whether a call would reach anyone (see ``III.is_observed``)."""
        return self._client.is_observed(self.id, call)

    def unregister(self) -> None:
        """Removes the function from the engine; ``on_observer_removed`` runs
        for each of its accepted observers."""
        self._unregister()

    def __repr__(self) -> str:
        return f"FunctionRef(id={self.id!r})"
