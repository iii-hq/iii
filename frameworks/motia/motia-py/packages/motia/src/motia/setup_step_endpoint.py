"""Step endpoint for tooling integration."""

import json
import logging
from pathlib import Path
from typing import Any

from .loader import generate_step_id

log = logging.getLogger("motia.step_endpoint")


def setup_step_endpoint(iii: Any) -> None:
    """Set up the step content endpoint for tooling.

    This registers a GET __motia/step/:stepId handler that allows
    developer tools, tutorials, and documentation to fetch step
    content and metadata.

    Args:
        iii: The III SDK instance
    """
    from .cli import discover_steps

    step_files = discover_steps("steps", include_src=True)
    step_map: dict[str, str] = {}

    for file_path in step_files:
        step_id = generate_step_id(file_path)
        step_map[step_id] = file_path
        log.debug(f"Mapped step {step_id} -> {file_path}")

    async def get_step_handler(req: dict[str, Any]) -> dict[str, Any]:
        step_id = req.get("path_params", {}).get("stepId")

        if not step_id:
            return {
                "status_code": 400,
                "body": {"error": "stepId is required"},
            }

        file_path = step_map.get(step_id)

        if not file_path:
            return {
                "status_code": 404,
                "body": {"error": "Step not found"},
            }

        try:
            content = Path(file_path).read_text()
            features_path = file_path.replace("/src/", "/tutorial/") + "-features.json"
            features = []
            if Path(features_path).exists():
                try:
                    features = json.loads(Path(features_path).read_text())
                except Exception as exc:
                    log.debug("Failed to read features file %s: %s", features_path, exc)
            return {
                "status_code": 200,
                "body": {
                    "id": step_id,
                    "content": content,
                    "features": features,
                },
            }
        except Exception as e:
            log.error(f"Error reading step file {file_path}: {e}")
            return {
                "status_code": 500,
                "body": {"error": f"Failed to read step: {e}"},
            }

    function_id = "motia_step_get"
    iii.register_function({"id": function_id}, get_step_handler)
    iii.register_trigger(
        {
            "type": "http",
            "function_id": function_id,
            "config": {"api_path": "__motia/step/:stepId", "http_method": "GET"},
        }
    )
    log.info("Registered step endpoint: GET __motia/step/:stepId")
