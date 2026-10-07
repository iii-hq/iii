"""Stream interface for the III SDK."""

from __future__ import annotations

import warnings
from abc import ABC, abstractmethod
from typing import Any, Generic, List, TypeVar

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

_ISTREAM_DEPRECATION = (
    "IStream is deprecated (iii-stream) and will be removed in a future release (version TBD). "
    "Behavior is unchanged for now. Migration guide: https://iii.dev/docs/upgrading/migrate-from-streams"
)


class IStream(ABC, Generic[TData]):
    """Abstract interface for stream operations.

    .. deprecated::
        IStream is deprecated (iii-stream) and will be removed in a future release (version TBD).
        Behavior is unchanged for now. Migration guide: https://iii.dev/docs/upgrading/migrate-from-streams

    Defining a subclass emits a :class:`FutureWarning` attributed to the line of the
    ``class`` statement (once per subclass definition, never per request). Importing
    this module does not warn.
    """

    def __init_subclass__(cls, **kwargs: Any) -> None:
        super().__init_subclass__(**kwargs)
        # stacklevel=3: __init_subclass__ <- ABCMeta.__new__ <- the user's class statement.
        warnings.warn(_ISTREAM_DEPRECATION, FutureWarning, stacklevel=3)

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
