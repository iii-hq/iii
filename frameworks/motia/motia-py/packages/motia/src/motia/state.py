"""State management for Motia framework."""

from __future__ import annotations

from typing import Any

from .iii import get_instance
from .tracing import operation_span, record_exception, set_span_ok

_list = list  # module-level alias; StateManager.list() shadows the builtin inside the class


class StateManager:
    """Internal state manager using state SDK calls."""

    def get(self, scope: str, key: str) -> Any | None:
        """Get a value from the state."""
        with operation_span(
            "state::get",
            **{"motia.state.scope": scope, "motia.state.key": key},
        ) as span:
            try:
                result = get_instance().trigger(
                    {
                        "function_id": "state::get",
                        "payload": {"scope": scope, "key": key},
                    }
                )
                set_span_ok(span)
                return result
            except Exception as exc:
                record_exception(span, exc)
                raise

    def set(self, scope: str, key: str, value: Any) -> Any:
        """Set a value in the state."""
        with operation_span(
            "state::set",
            **{"motia.state.scope": scope, "motia.state.key": key},
        ) as span:
            try:
                result = get_instance().trigger(
                    {
                        "function_id": "state::set",
                        "payload": {"scope": scope, "key": key, "value": value},
                    }
                )
                set_span_ok(span)
                return result
            except Exception as exc:
                record_exception(span, exc)
                raise

    def update(self, scope: str, key: str, ops: list[dict[str, Any]]) -> Any:
        """Update a value in the state using update operations."""
        with operation_span(
            "state::update",
            **{"motia.state.scope": scope, "motia.state.key": key},
        ) as span:
            try:
                result = get_instance().trigger(
                    {
                        "function_id": "state::update",
                        "payload": {"scope": scope, "key": key, "ops": ops},
                    }
                )
                set_span_ok(span)
                return result
            except Exception as exc:
                record_exception(span, exc)
                raise

    def delete(self, scope: str, key: str) -> Any | None:
        """Delete a value from the state."""
        with operation_span(
            "state::delete",
            **{"motia.state.scope": scope, "motia.state.key": key},
        ) as span:
            try:
                result = get_instance().trigger(
                    {
                        "function_id": "state::delete",
                        "payload": {"scope": scope, "key": key},
                    }
                )
                set_span_ok(span)
                return result
            except Exception as exc:
                record_exception(span, exc)
                raise

    def list(self, scope: str) -> list[Any]:
        """List all values in a scope."""
        with operation_span(
            "state::list",
            **{"motia.state.scope": scope},
        ) as span:
            try:
                items: list[Any] = get_instance().trigger(
                    {
                        "function_id": "state::list",
                        "payload": {"scope": scope},
                    }
                )
                set_span_ok(span)
                return items
            except Exception as exc:
                record_exception(span, exc)
                raise

    def list_groups(self) -> _list[str]:
        """List all scope IDs."""
        with operation_span("state::list_groups") as span:
            try:
                groups: _list[str] = get_instance().trigger({"function_id": "state::list_groups", "payload": {}})
                set_span_ok(span)
                return groups
            except Exception as exc:
                record_exception(span, exc)
                raise

    def clear(self, scope: str) -> None:
        """Clear all values in a scope."""
        with operation_span(
            "state::clear",
            **{"motia.state.scope": scope},
        ) as span:
            try:
                items = self.list(scope)
                for item in items:
                    if isinstance(item, dict) and "id" in item:
                        self.delete(scope, item["id"])
                set_span_ok(span)
            except Exception as exc:
                record_exception(span, exc)
                raise


stateManager = StateManager()
