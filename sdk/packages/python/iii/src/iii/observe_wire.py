"""Builders and decoders for the observe messages (protocol.md §2,
``sdk/fixtures/protocol/observe``). Optional fields are left out, never sent as
``null``; decoders accept ``null`` for them and keep enum values they do not
know."""

from __future__ import annotations

import json
from collections.abc import Callable
from typing import TYPE_CHECKING, Any

from pydantic import ValidationError

from .iii_types import (
    ActivationWire,
    CallerWire,
    ErrorBody,
    FunctionTargetWire,
    ObserveMessage,
    ObserverAckMessage,
    ObserveSourceWire,
)

if TYPE_CHECKING:
    from .invocation_context import ActiveInvocation


def decode_caller(value: Any) -> dict[str, Any] | None:
    """The wire caller as a dict: non-string fields and a ``parent`` inside
    ``parent`` dropped; ``None`` when it is not an object."""
    if not isinstance(value, dict):
        return None
    try:
        return CallerWire.model_validate(value).model_dump(exclude_none=True)
    except ValidationError:
        return None


def decode_activation(value: Any) -> dict[str, Any] | None:
    if not isinstance(value, dict) or not isinstance(value.get("observer_id"), str):
        return None
    try:
        return ActivationWire.model_validate(value).model_dump(exclude_none=True)
    except ValidationError:
        return None


def decode_observe_depth(value: Any) -> int:
    """``observe_depth``: a non-negative integer, ``0`` when absent or malformed."""
    if isinstance(value, int) and not isinstance(value, bool) and value > 0:
        return value
    return 0


def decode_error(value: Any) -> dict[str, str] | None:
    if not isinstance(value, dict):
        return None
    body = ErrorBody.model_validate(value)
    return {"code": body.code, "message": body.message}


def strip_caller(caller: dict[str, Any] | None) -> dict[str, Any] | None:
    """Drops ``parent`` (one level only)."""
    if caller is None:
        return None
    return {k: v for k, v in caller.items() if k != "parent"}


def report_caller(caller: dict[str, Any] | None) -> dict[str, Any] | None:
    """The ``caller`` a report item carries: the call's ``function_id``,
    ``invocation_id`` and ``parent``, never the top-level ``worker_id`` /
    ``worker_name`` (the engine stamps those); ``None`` when nothing is left
    (a call made outside any handler)."""
    if caller is None:
        return None
    reported = {k: v for k, v in caller.items() if k not in ("worker_id", "worker_name")}
    return reported or None


def caller_for(ctx: ActiveInvocation) -> dict[str, Any]:
    """The caller an outbound call asserts from inside a handler: that handler,
    and its own caller one level up."""
    caller: dict[str, Any] = {"function_id": ctx.function_id, "invocation_id": ctx.invocation_id}
    parent = strip_caller(ctx.caller)
    if parent:
        caller["parent"] = parent
    return caller


def snapshot(value: Any, what: str) -> Any:
    """JSON's view of a value: what travels, with no caller-owned objects retained."""
    try:
        return json.loads(json.dumps(value, allow_nan=False))
    except (TypeError, ValueError) as error:
        raise TypeError(f"{what} must be JSON: {error}") from None


NamespaceResolver = Callable[[Any, str, str], Any]


def build_observe_message(
    observer_id: str,
    source: dict[str, Any],
    target: Any,
    options: dict[str, Any] | None,
    resolve_namespace: NamespaceResolver,
) -> ObserveMessage:
    """``observe {id, source, target, ...}`` from the three arguments of
    ``iii.observe``. Only what the caller set is sent; the engine applies the
    defaults. ``resolve_namespace(explicit, function_id, field)`` gives the
    namespace of an unqualified id (this worker's, ``default`` for ``engine::*``).
    """
    src = snapshot(source, "observe: source")
    target_ref = {"function_id": target} if isinstance(target, str) else snapshot(target, "observe: target")
    opts = snapshot(options or {}, "observe: options")
    return ObserveMessage(
        id=observer_id,
        source=ObserveSourceWire(
            function_id=src["function_id"],
            namespace=resolve_namespace(src.get("namespace"), src["function_id"], "ObserveSource.namespace"),
            args=src.get("args"),
            filters=src.get("filters"),
            capture=src.get("capture"),
            on=src.get("on"),
            deliver=src.get("deliver"),
        ),
        target=FunctionTargetWire(
            function_id=target_ref["function_id"],
            namespace=resolve_namespace(target_ref.get("namespace"), target_ref["function_id"], "Target.namespace"),
        ),
        metadata=opts.get("metadata"),
        max_fires=opts.get("max_fires"),
        expires_in_ms=opts.get("expires_in_ms"),
        max_in_flight=opts.get("max_in_flight"),
    )


def build_observer_ack(
    function_id: str, namespace: str | None, observer_id: str, revision: int, error: dict[str, str] | None = None
) -> ObserverAckMessage:
    return ObserverAckMessage(
        function_id=function_id,
        namespace=namespace,
        id=observer_id,
        revision=revision,
        error=ErrorBody(code=error["code"], message=error["message"]) if error else None,
    )
