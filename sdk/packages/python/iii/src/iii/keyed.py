"""Keyed delivery: an ``on_observer_deliver`` hook that files observers under
keys computed from their args and hands each call to the observers filed
under the call's keys."""

from __future__ import annotations

from collections.abc import Callable, Iterable, Sequence
from typing import TYPE_CHECKING, Any, Union

if TYPE_CHECKING:
    from .observable import Observer
    from .observe_types import Call, Next

#: What a key function returns: one key, or several.
Keys = Union[str, Iterable[str]]


def _normalize(keys: Keys, what: str) -> tuple[str, ...]:
    if isinstance(keys, str):
        return (keys,)
    if isinstance(keys, (bytes, bytearray)) or not isinstance(keys, Iterable):
        raise TypeError(f"keyed: the {what} key function must return a string or strings, got {type(keys).__name__}")
    out = tuple(keys)
    for key in out:
        if not isinstance(key, str):
            raise TypeError(f"keyed: the {what} key function returned a {type(key).__name__} key")
    return out


class Keyed:
    """The deliver hook :func:`keyed` returns.

    Set on a function (``on_observer_deliver``), the SDK computes an
    observer's keys once, when it is accepted, files the observer under each,
    and unfiles it when it is removed. A call then reaches the observers filed
    under any of its keys, each at most once, through ``next``: their filters,
    ``on``, policy and ``deliver`` apply.

    Called directly as a hook (``keyed_hook(call, observers, next)``), it
    picks from ``observers`` by the same keys.
    """

    def __init__(self, observer: Callable[[Observer], Keys], call: Callable[[Call[Any, Any]], Keys]) -> None:
        self._observer = observer
        self._call = call

    def observer_keys(self, obs: Observer) -> tuple[str, ...]:
        """The keys ``obs`` is filed under."""
        return _normalize(self._observer(obs), "observer")

    def call_keys(self, call: Call[Any, Any]) -> tuple[str, ...]:
        """The keys ``call`` is delivered to."""
        return _normalize(self._call(call), "call")

    def __call__(self, call: Call[Any, Any], observers: Sequence[Observer], next: Next) -> Any:
        wanted = set(self.call_keys(call))
        chosen = [obs for obs in observers if wanted.intersection(self.observer_keys(obs))]
        return next(call, chosen)

    def __repr__(self) -> str:
        return f"keyed(observer={self._observer!r}, call={self._call!r})"


def keyed(*, observer: Callable[[Observer], Keys], call: Callable[[Call[Any, Any]], Keys]) -> Keyed:
    """Keyed delivery for a function with many observers: an index from keys
    to observers instead of a scan of every observer on every call.

    Each function returns a string or several. Wildcards are keys too: file an
    observer of every table under ``"primary|*|insert"`` and deliver a change
    to ``"primary|orders|insert"``, ``"primary|orders|*"``,
    ``"primary|*|insert"`` and ``"primary|*|*"``. On a keyed function,
    ``is_observed(call)`` is true when an observer is filed under one of the
    call's keys.

    Args:
        observer: The key or keys an observer is filed under, from its args.
        call: The key or keys a call is delivered to.

    Examples:
        >>> from iii import keyed
        >>> iii.on_observer_deliver("cron::on-tick", keyed(
        ...     observer=lambda obs: obs.args["expression"],
        ...     call=lambda call: call.input["expression"],
        ... ))
    """
    return Keyed(observer, call)
