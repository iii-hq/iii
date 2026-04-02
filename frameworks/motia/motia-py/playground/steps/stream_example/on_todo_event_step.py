"""Step that reacts to stream events."""

from typing import Any

from motia import StreamTriggerInput, enqueue, logger, stream

config = {
    "name": "on-todo-stream-event",
    "description": "React to todo stream events",
    "triggers": [
        stream("todo"),
    ],
    "enqueues": ["todo.processed"],
    "flows": ["stream-example"],
}


def handler(input: StreamTriggerInput) -> None:
    """Handle todo stream event."""
    logger.info(
        f"Todo stream event",
        {
            "stream_name": input.stream_name,
            "group_id": input.group_id,
            "item_id": input.id,
            "event_type": input.event.type,
            "data": input.event.data,
        },
    )

    if input.event.type == "create":
        logger.info(f"New todo created: {input.id}")
    elif input.event.type == "update":
        logger.info(f"Todo updated: {input.id}")
    elif input.event.type == "delete":
        logger.info(f"Todo deleted: {input.id}")

    enqueue(
        {
            "topic": "todo.processed",
            "data": {
                "todo_id": input.id,
                "event_type": input.event.type,
            },
        }
    )
