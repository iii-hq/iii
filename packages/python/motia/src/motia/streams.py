"""Stream implementation for Motia framework."""

import logging
from typing import TYPE_CHECKING, Any, Generic, TypeVar

if TYPE_CHECKING:
    from iii import Bridge

TData = TypeVar("TData")
log = logging.getLogger("motia.streams")


class Stream(Generic[TData]):
    """Stream for managing distributed state."""

    def __init__(self, stream_name: str, bridge: "Bridge | None" = None) -> None:
        self.stream_name = stream_name
        self._bridge = bridge
        log.debug(f"Stream created: {stream_name}")

    def _get_bridge(self) -> Any:
        """Get the bridge instance."""
        if self._bridge is None:
            from .bridge import bridge
            self._bridge = bridge
        return self._bridge

    async def get(self, group_id: str, item_id: str) -> TData | None:
        """Get an item from the stream."""
        return await self._get_bridge().invoke_function(
            "streams.get",
            {
                "stream_name": self.stream_name,
                "group_id": group_id,
                "item_id": item_id,
            },
        )

    async def set(self, group_id: str, item_id: str, data: TData) -> TData:
        """Set an item in the stream."""
        return await self._get_bridge().invoke_function(
            "streams.set",
            {
                "stream_name": self.stream_name,
                "group_id": group_id,
                "item_id": item_id,
                "data": data,
            },
        )

    async def delete(self, group_id: str, item_id: str) -> None:
        """Delete an item from the stream."""
        await self._get_bridge().invoke_function(
            "streams.delete",
            {
                "stream_name": self.stream_name,
                "group_id": group_id,
                "item_id": item_id,
            },
        )

    async def get_group(self, group_id: str) -> list[TData]:
        """Get all items in a group."""
        return await self._get_bridge().invoke_function(
            "streams.getGroup",
            {
                "stream_name": self.stream_name,
                "group_id": group_id,
            },
        )
