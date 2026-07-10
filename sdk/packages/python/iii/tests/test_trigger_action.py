"""Contract test for the SDK-owned TriggerAction builders.

The queue *behavior* (delivery, ordering, DLQ, durable fan-out) is exercised by
the standalone queue worker's own suite; here we only pin the wire shape the
engine's ``TriggerAction`` deserialization depends on, so it needs no live engine.
"""

from iii.iii_types import TriggerActionEnqueue, TriggerActionVoid


def test_enqueue_serializes_to_type_and_queue() -> None:
    assert TriggerActionEnqueue(queue="orders").model_dump() == {
        "type": "enqueue",
        "queue": "orders",
    }


def test_void_serializes_to_type() -> None:
    assert TriggerActionVoid().model_dump() == {"type": "void"}
