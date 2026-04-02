"""Standalone enqueue function for Motia framework."""

from typing import Any

from .iii import get_instance
from .tracing import operation_span


def enqueue(event: dict[str, Any]) -> None:
    """Enqueue an event to a topic."""
    with operation_span("enqueue", **{"motia.step.name": ""}):
        get_instance().trigger({"function_id": "enqueue", "payload": event})
