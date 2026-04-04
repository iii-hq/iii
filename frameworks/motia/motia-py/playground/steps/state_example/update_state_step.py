"""Step that updates state to trigger state-listening steps."""

from typing import Any

from motia import http, logger, stateManager

config = {
    "name": "update-user-state",
    "description": "API endpoint to update user state",
    "triggers": [
        http("POST", "/users/:id/status"),
    ],
    "flows": ["state-example"],
}


def handler(request: Any) -> dict[str, Any]:
    """Update user status in state."""
    # Validate path_params contains "id"
    if not request.path_params or "id" not in request.path_params:
        return {
            "status": 400,
            "body": {"error": "Missing required path parameter: id"},
        }

    user_id = request.path_params.get("id")

    # Validate request.body is a mapping
    if request.body is None or not hasattr(request.body, "get"):
        return {
            "status": 400,
            "body": {"error": "Request body must be a valid object"},
        }

    new_status = request.body.get("status", "active")

    stateManager.set("users", user_id, {"status": new_status})

    logger.info(f"Updated user {user_id} status to {new_status}")

    return {
        "status": 200,
        "body": {"user_id": user_id, "status": new_status},
    }
