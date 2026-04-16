# Data Worker - Data validation and transformation
# Demonstrates: register_function with Pydantic validation

import os
from iii import register_worker, InitOptions, Logger
from pydantic import BaseModel, ValidationError

class TransformInput(BaseModel):
    data: dict

iii = register_worker(
    os.environ.get("III_URL", "ws://localhost:49134"),
    InitOptions(worker_name="data-worker")
)
logger = Logger()

def transform_handler(payload: dict) -> dict:
    try:
        validated = TransformInput.model_validate(payload)
    except ValidationError as e:
        logger.error(f"Validation error: {e}")
        return {"error": "Invalid payload", "details": e.errors()}

    worker_version = iii.trigger({"function_id": "state::get", "payload": {"scope": "shared", "key": "WORKER_VERSION"}})

    logger.info("Processing data with data-worker...")

    return {
        "transformed": validated.data,
        "keys": list(validated.data.keys()),
        "source": "data-worker",
        "worker-version": f"worker version {worker_version}"
    }

iii.register_function("data-worker::transform", transform_handler)

print("Data worker started - listening for calls")
