"""State management for Motia framework."""

from typing import Any, TypeVar

TData = TypeVar("TData")

STREAM_NAME = "$$internal-state"


class StateManager:
    """Internal state manager using streams."""

    def __init__(self) -> None:
        self._bridge: Any = None

    def _get_bridge(self) -> Any:
        """Lazy load bridge to avoid circular imports."""
        if self._bridge is None:
            from .bridge import bridge

            self._bridge = bridge
        return self._bridge

    async def get(self, group_id: str, item_id: str) -> Any | None:
        """Get a value from the state."""
        return await self._get_bridge().invoke_function(
            "streams.get",
            {
                "stream_name": STREAM_NAME,
                "group_id": group_id,
                "item_id": item_id,
            },
        )

    async def set(self, group_id: str, item_id: str, data: Any) -> Any:
        """Set a value in the state."""
        return await self._get_bridge().invoke_function(
            "streams.set",
            {
                "stream_name": STREAM_NAME,
                "group_id": group_id,
                "item_id": item_id,
                "data": data,
            },
        )

    async def delete(self, group_id: str, item_id: str) -> Any | None:
        """Delete a value from the state."""
        return await self._get_bridge().invoke_function(
            "streams.delete",
            {
                "stream_name": STREAM_NAME,
                "group_id": group_id,
                "item_id": item_id,
            },
        )

    async def get_group(self, group_id: str) -> list[Any]:
        """Get all values in a group."""
        return await self._get_bridge().invoke_function(
            "streams.getGroup",
            {
                "stream_name": STREAM_NAME,
                "group_id": group_id,
            },
        )

    async def clear(self, group_id: str) -> None:
        """Clear all values in a group."""
        items = await self.get_group(group_id)
        for item in items:
            if isinstance(item, dict) and "id" in item:
                await self.delete(group_id, item["id"])
