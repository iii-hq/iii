"""Stream types and interfaces for the III SDK."""

from abc import ABC, abstractmethod
from typing import Any, Generic, TypeVar

from pydantic import BaseModel

TData = TypeVar("TData")


class StreamAuthInput(BaseModel):
    """Input for stream authentication."""

    headers: dict[str, str]
    path: str
    query_params: dict[str, list[str]]
    addr: str


class StreamAuthResult(BaseModel):
    """Result of stream authentication."""

    context: Any | None = None


class StreamJoinLeaveEvent(BaseModel):
    """Event for stream join/leave."""

    subscription_id: str
    stream_name: str
    group_id: str
    id: str | None = None
    context: Any | None = None


class StreamJoinResult(BaseModel):
    """Result of stream join."""

    unauthorized: bool


class StreamGetInput(BaseModel):
    """Input for stream get operation."""

    stream_name: str
    group_id: str
    item_id: str


class StreamSetInput(BaseModel):
    """Input for stream set operation."""

    stream_name: str
    group_id: str
    item_id: str
    data: Any


class StreamDeleteInput(BaseModel):
    """Input for stream delete operation."""

    stream_name: str
    group_id: str
    item_id: str


class StreamGetGroupInput(BaseModel):
    """Input for stream get group operation."""

    stream_name: str
    group_id: str


class StreamListGroupsInput(BaseModel):
    """Input for stream list groups operation."""

    stream_name: str


class StreamUpdateInput(BaseModel):
    """Input for stream update operation."""

    stream_name: str
    group_id: str
    item_id: str
    ops: list["UpdateOp"]


class StreamSetResult(BaseModel, Generic[TData]):
    """Result of stream set operation."""

    old_value: TData | None = None
    new_value: TData | None = None


class UpdateSet(BaseModel):
    """Set operation for stream update."""

    type: str = "set"
    path: str
    value: Any


class UpdateIncrement(BaseModel):
    """Increment operation for stream update."""

    type: str = "increment"
    path: str
    by: int | float


class UpdateDecrement(BaseModel):
    """Decrement operation for stream update."""

    type: str = "decrement"
    path: str
    by: int | float


class UpdateRemove(BaseModel):
    """Remove operation for stream update."""

    type: str = "remove"
    path: str


class UpdateMerge(BaseModel):
    """Merge operation for stream update."""

    type: str = "merge"
    path: str
    value: Any


UpdateOp = UpdateSet | UpdateIncrement | UpdateDecrement | UpdateRemove | UpdateMerge


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
    async def delete(self, input: StreamDeleteInput) -> None:
        """Delete an item from the stream."""
        ...

    @abstractmethod
    async def get_group(self, input: StreamGetGroupInput) -> list[TData]:
        """Get all items in a group."""
        ...

    @abstractmethod
    async def list_groups(self, input: StreamListGroupsInput) -> list[str]:
        """List all groups in the stream."""
        ...

    @abstractmethod
    async def update(self, input: StreamUpdateInput) -> StreamSetResult[TData] | None:
        """Update an item in the stream."""
        ...
