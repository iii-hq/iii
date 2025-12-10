"""Stream type definitions."""

from pydantic import BaseModel


class StreamConfig(BaseModel):
    """Configuration for a stream."""

    name: str
    description: str | None = None
