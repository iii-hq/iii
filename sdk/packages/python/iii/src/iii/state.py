"""State management types and interface for the III SDK.

Mirrors the Node SDK ``iii-sdk/state`` subpath export.
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from enum import Enum
from typing import Any, Generic, TypeVar

from pydantic import BaseModel

from .stream import UpdateOp

TData = TypeVar("TData")


class StateGetInput(BaseModel):
    """Input for retrieving a state value."""

    scope: str
    key: str


class StateSetInput(BaseModel):
    """Input for setting a state value."""

    scope: str
    key: str
    value: Any


class StateDeleteInput(BaseModel):
    """Input for deleting a state value."""

    scope: str
    key: str


class StateListInput(BaseModel):
    """Input for listing all values in a state scope."""

    scope: str


class StateUpdateInput(BaseModel):
    """Input for atomically updating a state value."""

    scope: str
    key: str
    ops: list[UpdateOp]


class StateSetResult(BaseModel, Generic[TData]):
    """Result of a state set operation."""

    old_value: TData | None = None
    new_value: TData


class StateUpdateResult(BaseModel, Generic[TData]):
    """Result of a state update operation."""

    old_value: TData | None = None
    new_value: TData


class StateDeleteResult(BaseModel):
    """Result of a state delete operation."""

    old_value: Any | None = None


class DeleteResult(BaseModel):
    """Result of a state delete operation (alias)."""

    old_value: Any | None = None


class StateEventType(str, Enum):
    """Types of state change events."""

    CREATED = "state:created"
    UPDATED = "state:updated"
    DELETED = "state:deleted"


class StateEventData(BaseModel, Generic[TData]):
    """Payload for state change events."""

    type: str = "state"
    event_type: StateEventType
    scope: str
    key: str
    old_value: TData | None = None
    new_value: TData | None = None


class IState(ABC, Generic[TData]):
    """Abstract interface for state management operations."""

    @abstractmethod
    async def get(self, input: StateGetInput) -> TData | None:
        """Retrieve a value by scope and key."""
        ...

    @abstractmethod
    async def set(self, input: StateSetInput) -> StateSetResult[TData] | None:
        """Set (create or overwrite) a state value."""
        ...

    @abstractmethod
    async def delete(self, input: StateDeleteInput) -> DeleteResult:
        """Delete a state value."""
        ...

    @abstractmethod
    async def list(self, input: StateListInput) -> list[TData]:
        """List all values in a scope."""
        ...

    @abstractmethod
    async def update(self, input: StateUpdateInput) -> StateUpdateResult[TData] | None:
        """Apply atomic update operations to a state value."""
        ...
