"""iii queue helpers."""

from pydantic import BaseModel, Field


class EnqueueResult(BaseModel):
    """Result returned when a function is invoked with ``TriggerAction.Enqueue``.

    Attributes:
        messageReceiptId: UUID assigned by the engine to the enqueued job.
    """

    messageReceiptId: str = Field(description="UUID assigned by the engine to the enqueued job.")


__all__ = [
    "EnqueueResult",
]
