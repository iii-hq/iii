"""iii queue helpers."""

from pydantic import BaseModel, Field


class EnqueueResult(BaseModel):
    """Result returned when a function is invoked with ``TriggerAction.Enqueue``.

    Attributes:
        messageReceiptId: Unique receipt ID for the enqueued message.
    """

    messageReceiptId: str = Field(description="Unique receipt ID for the enqueued message.")


__all__ = [
    "EnqueueResult",
]
