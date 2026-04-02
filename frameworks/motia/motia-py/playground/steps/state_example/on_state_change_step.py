"""Step that reacts to state changes."""

from typing import Any

from motia import StateTriggerInput, enqueue, logger, state

config = {
    "name": "on-user-state-change",
    "description": "React to user state changes",
    "triggers": [
        state(condition=lambda input, ctx: input.group_id == "users"),
    ],
    "enqueues": ["user.status.changed"],
    "flows": ["state-example"],
}


def handler(input: StateTriggerInput) -> None:
    """Handle user state change."""
    logger.info(
        f"User state changed",
        {
            "group_id": input.group_id,
            "item_id": input.item_id,
            "old_value": input.old_value,
            "new_value": input.new_value,
        },
    )

    old_status = input.old_value.get("status") if isinstance(input.old_value, dict) else None
    new_status = input.new_value.get("status") if isinstance(input.new_value, dict) else None

    enqueue(
        {
            "topic": "user.status.changed",
            "data": {
                "user_id": input.item_id,
                "old_status": old_status,
                "new_status": new_status,
            },
        }
    )
