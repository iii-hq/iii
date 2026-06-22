"""Stream interface for the III SDK."""

from __future__ import annotations

from abc import ABC, abstractmethod
from typing import Generic, List, TypeVar

from iii_helpers.stream import (
    StreamDeleteInput,
    StreamDeleteResult,
    StreamGetInput,
    StreamListGroupsInput,
    StreamListInput,
    StreamSetInput,
    StreamSetResult,
    StreamUpdateInput,
    StreamUpdateResult,
)

TData = TypeVar("TData")


class IStream(ABC, Generic[TData]):
    """Abstract interface for stream operations."""

    @abstractmethod
    async def get(self, input: StreamGetInput) -> TData | None:
        """Get an item from the stream."""
        ...

    @abstractmethod
    async def set(self, input: StreamSetInput) -> StreamSetResult[TData] | None:
        """Set an item in the stream."""
        ...

    @abstractmethod
    async def delete(self, input: StreamDeleteInput) -> StreamDeleteResult:
        """Delete an item from the stream."""
        ...

    @abstractmethod
    async def list(self, input: StreamListInput) -> list[TData]:
        """Get all items in a group."""
        ...

    @abstractmethod
    async def list_groups(self, input: StreamListGroupsInput) -> List[str]:
        """List all groups in the stream."""
        ...

    @abstractmethod
    async def update(self, input: StreamUpdateInput) -> StreamUpdateResult[TData] | None:
        """Apply atomic update operations to a stream item."""
        ...
