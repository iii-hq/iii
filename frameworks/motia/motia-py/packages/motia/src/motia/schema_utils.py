"""Schema conversion utilities."""

from typing import Any

from pydantic import BaseModel


def schema_to_json_schema(schema: Any) -> dict[str, Any] | None:
    """Convert supported schema types to JSON Schema dict."""
    if schema is None:
        return None
    if isinstance(schema, dict):
        return schema
    if isinstance(schema, type) and issubclass(schema, BaseModel):
        return schema.model_json_schema()
    return None
