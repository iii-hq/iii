"""Contract test for the SDK-owned CallAction builders.

The queue *behavior* (delivery, ordering, DLQ, durable fan-out) is exercised by
the standalone queue worker's own suite; here we only pin the wire shape the
engine's ``CallAction`` deserialization depends on, so it needs no live engine.
"""

from iii import CallAction, CallActionEnqueue, CallActionVoid, TriggerAction, TriggerActionEnqueue, TriggerActionVoid


def test_enqueue_serializes_to_type_and_queue() -> None:
    assert CallAction.Enqueue(queue="orders").model_dump() == {
        "type": "enqueue",
        "queue": "orders",
    }


def test_void_serializes_to_type() -> None:
    assert CallAction.Void().model_dump() == {"type": "void"}


def test_the_0_25_names_are_the_same_types() -> None:
    assert TriggerAction is CallAction
    assert TriggerActionEnqueue is CallActionEnqueue
    assert TriggerActionVoid is CallActionVoid
