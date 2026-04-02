"""Schema validation utilities for Motia framework."""

import logging
from typing import Any

from pydantic import BaseModel, ValidationError

from .types import StepConfig

log = logging.getLogger("motia.validator")


def validate_schema(schema: dict[str, Any]) -> list[str]:
    """Validate a JSON Schema structure.

    Args:
        schema: The JSON Schema to validate

    Returns:
        List of validation errors (empty if valid)
    """
    errors: list[str] = []

    if not isinstance(schema, dict):
        errors.append("Schema must be a dictionary")
        return errors

    # Check for required JSON Schema properties
    if "type" not in schema and "$ref" not in schema and "anyOf" not in schema and "oneOf" not in schema:
        errors.append("Schema should have a 'type', '$ref', 'anyOf', or 'oneOf' property")

    schema_type = schema.get("type")

    if schema_type == "object":
        properties = schema.get("properties")
        if properties is not None and not isinstance(properties, dict):
            errors.append("'properties' must be a dictionary")

        required = schema.get("required")
        if required is not None and not isinstance(required, list):
            errors.append("'required' must be a list")

    elif schema_type == "array":
        items = schema.get("items")
        if items is not None and not isinstance(items, dict):
            errors.append("'items' must be a dictionary")

    return errors


def validate_step_config(config: Any) -> list[str]:
    """Validate a step configuration.

    Args:
        config: The step configuration to validate

    Returns:
        List of validation errors (empty if valid)
    """
    errors: list[str] = []

    if not hasattr(config, "name"):
        errors.append("Step config must have a 'name' property")
        return errors

    if not config.name:
        errors.append("Step name cannot be empty")

    if not hasattr(config, "triggers") or not config.triggers:
        errors.append("Step must have at least one trigger")

    return errors


def validate_step(config: StepConfig) -> list[str]:
    """Validate a step configuration with Pydantic."""
    errors: list[str] = []
    try:
        StepConfig.model_validate(config)
    except ValidationError as exc:
        errors.extend([e.get("msg", "Invalid step config") for e in exc.errors()])
    return errors


def pydantic_to_json_schema(model: type[BaseModel]) -> dict[str, Any]:
    """Convert a Pydantic model to JSON Schema.

    Args:
        model: The Pydantic model class

    Returns:
        JSON Schema dictionary
    """
    return model.model_json_schema()


def check_schema_compatibility(
    schema1: dict[str, Any],
    schema2: dict[str, Any],
) -> list[str]:
    """Check if two schemas are compatible for event topics.

    This performs a basic compatibility check between schemas
    that share the same event topic.

    Args:
        schema1: The first schema
        schema2: The second schema

    Returns:
        List of compatibility issues (empty if compatible)
    """
    issues: list[str] = []

    type1 = schema1.get("type")
    type2 = schema2.get("type")

    if type1 != type2:
        issues.append(f"Type mismatch: {type1} vs {type2}")

    if type1 == "object" and type2 == "object":
        props1 = set(schema1.get("properties", {}).keys())
        props2 = set(schema2.get("properties", {}).keys())

        # Check for required fields that don't exist in the other schema
        req1 = set(schema1.get("required", []))
        req2 = set(schema2.get("required", []))

        missing_in_2 = req1 - props2
        missing_in_1 = req2 - props1

        if missing_in_2:
            issues.append(f"Required fields missing in second schema: {missing_in_2}")
        if missing_in_1:
            issues.append(f"Required fields missing in first schema: {missing_in_1}")

    return issues
