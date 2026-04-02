"""Stream implementation for Motia framework."""

from __future__ import annotations

import logging
from typing import TYPE_CHECKING, Any, Generic, TypeVar

from .iii import get_instance
from .tracing import operation_span, record_exception, set_span_ok

if TYPE_CHECKING:
    from .types_stream import StreamConfig

TData = TypeVar("TData")
_list = list  # module-level alias; Stream.list() shadows the builtin inside the class
log = logging.getLogger("motia.streams")


class Stream(Generic[TData]):
    """Stream for managing distributed state."""

    def __init__(self, config: "StreamConfig | str") -> None:
        from .types_stream import StreamConfig

        if isinstance(config, str):
            self.stream_name = config
            self.config: StreamConfig = StreamConfig(name=config)
        else:
            self.stream_name = config.name
            self.config = config
        log.debug(f"Stream created: {self.stream_name}")

    def get(self, group_id: str, item_id: str) -> TData | None:
        """Get an item from the stream."""
        with operation_span(
            "stream::get",
            **{
                "motia.stream.name": self.stream_name,
                "motia.stream.group_id": group_id,
                "motia.stream.item_id": item_id,
            },
        ) as span:
            try:
                value: TData | None = get_instance().trigger(
                    {
                        "function_id": "stream::get",
                        "payload": {
                            "stream_name": self.stream_name,
                            "group_id": group_id,
                            "item_id": item_id,
                        },
                    }
                )
                set_span_ok(span)
                return value
            except Exception as exc:
                record_exception(span, exc)
                raise

    def set(self, group_id: str, item_id: str, data: TData) -> Any:
        """Set an item in the stream."""
        with operation_span(
            "stream::set",
            **{
                "motia.stream.name": self.stream_name,
                "motia.stream.group_id": group_id,
                "motia.stream.item_id": item_id,
            },
        ) as span:
            try:
                result: Any = get_instance().trigger(
                    {
                        "function_id": "stream::set",
                        "payload": {
                            "stream_name": self.stream_name,
                            "group_id": group_id,
                            "item_id": item_id,
                            "data": data,
                        },
                    }
                )
                set_span_ok(span)
                return result
            except Exception as exc:
                record_exception(span, exc)
                raise

    def delete(self, group_id: str, item_id: str) -> None:
        """Delete an item from the stream."""
        with operation_span(
            "stream::delete",
            **{
                "motia.stream.name": self.stream_name,
                "motia.stream.group_id": group_id,
                "motia.stream.item_id": item_id,
            },
        ) as span:
            try:
                get_instance().trigger(
                    {
                        "function_id": "stream::delete",
                        "payload": {
                            "stream_name": self.stream_name,
                            "group_id": group_id,
                            "item_id": item_id,
                        },
                    }
                )
                set_span_ok(span)
            except Exception as exc:
                record_exception(span, exc)
                raise

    def get_group(self, group_id: str) -> list[TData]:
        """Get all items in a group."""
        with operation_span(
            "stream::list",
            **{
                "motia.stream.name": self.stream_name,
                "motia.stream.group_id": group_id,
            },
        ) as span:
            try:
                items: list[TData] = get_instance().trigger(
                    {
                        "function_id": "stream::list",
                        "payload": {
                            "stream_name": self.stream_name,
                            "group_id": group_id,
                        },
                    }
                )
                set_span_ok(span)
                return items
            except Exception as exc:
                record_exception(span, exc)
                raise

    def list(self, group_id: str) -> list[TData]:
        """List all items in a group. Alias for get_group()."""
        return self.get_group(group_id)

    def update(self, group_id: str, item_id: str, ops: _list[dict[str, Any]]) -> Any:
        """Update an item in the stream using update operations."""
        with operation_span(
            "stream::update",
            **{
                "motia.stream.name": self.stream_name,
                "motia.stream.group_id": group_id,
                "motia.stream.item_id": item_id,
            },
        ) as span:
            try:
                result = get_instance().trigger(
                    {
                        "function_id": "stream::update",
                        "payload": {
                            "stream_name": self.stream_name,
                            "group_id": group_id,
                            "item_id": item_id,
                            "ops": ops,
                        },
                    }
                )
                set_span_ok(span)
                return result
            except Exception as exc:
                record_exception(span, exc)
                raise

    def list_groups(self) -> _list[str]:
        """List all group IDs for the stream."""
        with operation_span(
            "stream::list_groups",
            **{
                "motia.stream.name": self.stream_name,
            },
        ) as span:
            try:
                groups: _list[str] = get_instance().trigger(
                    {
                        "function_id": "stream::list_groups",
                        "payload": {
                            "stream_name": self.stream_name,
                        },
                    }
                )
                set_span_ok(span)
                return groups
            except Exception as exc:
                record_exception(span, exc)
                raise
