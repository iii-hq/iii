"""Step loader with stable step IDs."""

import uuid

# Namespace UUID for generating stable step IDs
# This is the same namespace used in the Node implementation
STEP_NAMESPACE = uuid.UUID("7f1c3ff2-9b00-4d0a-bdd7-efb8bca49d4f")


def generate_step_id(path: str) -> str:
    """Generate a stable step ID using uuid5.

    The step ID is deterministic based on the file path,
    enabling tooling (UIs, tutorials, docs) to reference
    steps across builds and refactors reliably.

    Args:
        path: The file path of the step

    Returns:
        A stable UUID string for the step
    """
    return str(uuid.uuid5(STEP_NAMESPACE, path))
