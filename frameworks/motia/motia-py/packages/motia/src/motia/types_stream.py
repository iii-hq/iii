"""Stream type definitions."""

from typing import Any, Awaitable, Callable

from pydantic import BaseModel, ConfigDict

from .types import FlowContext


class StreamSubscription(BaseModel):
    """Stream subscription details."""

    group_id: str
    id: str | None = None


class StreamAuthInput(BaseModel):
    """Authentication input for stream connections."""

    headers: dict[str, str]
    path: str
    query_params: dict[str, list[str]]
    addr: str


class StreamAuthResult(BaseModel):
    """Authentication result for stream connections."""

    authorized: bool
    context: Any | None = None


class StreamConfig(BaseModel):
    """Configuration for a stream."""

    model_config = ConfigDict(arbitrary_types_allowed=True)

    name: str
    description: str | None = None
    schema: dict[str, Any] | None = None  # type: ignore[assignment]
    base_config: dict[str, Any] | None = None
    on_join: Callable[[StreamSubscription, FlowContext[Any], Any | None], Awaitable[Any]] | None = None
    on_leave: Callable[[StreamSubscription, FlowContext[Any], Any | None], Awaitable[Any]] | None = None
