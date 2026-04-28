"""Stream types and interfaces for the III SDK."""

from __future__ import annotations

from abc import ABC, abstractmethod
from typing import Any, Generic, List, Literal, TypeVar

from pydantic import BaseModel, Field

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


StreamContext = Any


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


class StreamListInput(BaseModel):
    """Input for stream list operation."""

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


class UpdateOpError(BaseModel):
    """Per-op error returned by ``state::update`` / ``stream::update``.

    Currently emitted only by the ``merge`` op when input violates the
    new validation bounds. Successfully applied ops are still
    reflected in the response's ``new_value``.
    """

    op_index: int
    code: str
    message: str
    doc_url: str | None = None


class StreamSetResult(BaseModel, Generic[TData]):
    """Result of stream set operation."""

    old_value: TData | None = None
    new_value: TData


class StreamUpdateResult(BaseModel, Generic[TData]):
    """Result of stream update operation."""

    old_value: TData | None = None
    new_value: TData
    # Per-op errors. Currently emitted only by the ``merge`` op for
    # validation rejections. Field is omitted from the JSON wire when
    # empty. ``default_factory`` is used (not ``= []``) to keep
    # Pydantic's parameterized-Generic + default handling well-behaved
    # across Python versions.
    errors: list[UpdateOpError] = Field(default_factory=list)


class StreamDeleteResult(BaseModel):
    """Result of stream delete operation."""

    old_value: Any | None = None


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


class UpdateAppend(BaseModel):
    """Append operation for stream update."""

    type: str = "append"
    path: str
    value: Any


class UpdateRemove(BaseModel):
    """Remove operation for stream update."""

    type: str = "remove"
    path: str


class UpdateMerge(BaseModel):
    """Shallow merge an object into the target.

    The target is the root (when ``path`` is omitted, an empty string,
    or an empty list) or an arbitrary nested location specified by an
    array of literal segments.

    Path forms accepted:
      - ``None`` / ``""`` / ``[]``: merge at the root.
      - ``"foo"``: equivalent to ``["foo"]`` -- single first-level key.
      - ``["a", "b", "c"]``: nested path. Each element is a *literal*
        key. ``["a.b"]`` writes a single key named ``"a.b"``, not
        ``a -> b``.

    Engine semantics:
      - Missing or non-object intermediates along the path are
        auto-replaced with ``{}``.
      - The merge is shallow at the target node (top-level keys of
        ``value`` overwrite same-named keys; siblings preserved).

    Validation: invalid paths/values (depth > 32 segments, segment >
    256 bytes, value depth > 16, > 1024 top-level keys, or any
    ``__proto__`` / ``constructor`` / ``prototype`` segment or
    top-level key) are rejected with a structured error in the
    ``errors`` array of the ``state::update`` / ``stream::update``
    response. The merge does not apply when an error is returned.
    """

    type: str = "merge"
    # Optional. Accepts a single string or a list of literal segments.
    # Pydantic resolves ``str | list[str]`` via smart-union: string
    # input -> str, array input -> list[str].
    path: str | list[str] | None = None
    value: Any


UpdateOp = UpdateSet | UpdateIncrement | UpdateDecrement | UpdateAppend | UpdateRemove | UpdateMerge


class StreamTriggerConfig(BaseModel):
    """Trigger config for ``stream`` triggers. Filters which item changes fire the handler."""

    stream_name: str
    group_id: str | None = None
    item_id: str | None = None
    condition_function_id: str | None = None


class StreamJoinLeaveTriggerConfig(BaseModel):
    """Trigger config for ``stream:join`` and ``stream:leave`` triggers."""

    condition_function_id: str | None = None


class StreamChangeEventDetail(BaseModel):
    """Detail of a stream change event containing the mutation type and data."""

    type: Literal["create", "update", "delete"]
    data: Any


class StreamChangeEvent(BaseModel):
    """Handler input for ``stream`` triggers, fired when an item changes."""

    type: Literal["stream"]
    timestamp: int
    streamName: str
    groupId: str
    id: str | None = None
    event: StreamChangeEventDetail


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
