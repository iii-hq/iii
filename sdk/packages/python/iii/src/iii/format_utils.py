"""Utilities for extracting JSON Schema from Python type hints."""

from __future__ import annotations

import inspect
import types
import typing
from typing import Any, get_args, get_origin, get_type_hints

_PRIMITIVE_MAP: dict[type, str] = {
    str: "string",
    int: "integer",
    float: "number",
    bool: "boolean",
}


def _is_optional(annotation: Any) -> tuple[bool, Any]:
    """Check if a type is Optional[X] (i.e. Union[X, None]) and return (is_optional, inner_type)."""
    origin = get_origin(annotation)
    if origin is types.UnionType or origin is typing.Union:
        args = get_args(annotation)
        non_none = [a for a in args if a is not type(None)]
        if len(non_none) == 1 and len(args) == 2:
            return True, non_none[0]
    return False, annotation


def _is_pydantic_model(annotation: Any) -> bool:
    """Check if a type is a Pydantic BaseModel subclass."""
    try:
        from pydantic import BaseModel

        return isinstance(annotation, type) and issubclass(annotation, BaseModel)
    except ImportError:
        return False


_JSON_SCHEMA_DRAFT = "https://json-schema.org/draft/2020-12/schema"


def _to_json_schema(annotation: Any) -> dict[str, Any] | None:
    """Convert a Python type annotation to a JSON Schema dict (without $schema header)."""
    if annotation is inspect.Parameter.empty or annotation is Any:
        return None

    # Handle Optional[X] — produce {"type": ["<inner>", "null"]}
    is_opt, inner = _is_optional(annotation)
    if is_opt:
        inner_schema = _to_json_schema(inner)
        if inner_schema is not None and "type" in inner_schema:
            inner_schema["type"] = [inner_schema["type"], "null"]
            return inner_schema
        return inner_schema

    # Handle NoneType
    if annotation is type(None):
        return {"type": "null"}

    # Handle primitives
    if annotation in _PRIMITIVE_MAP:
        return {"type": _PRIMITIVE_MAP[annotation]}

    # Handle list[X]
    origin = get_origin(annotation)
    if origin is list:
        args = get_args(annotation)
        schema: dict[str, Any] = {"type": "array"}
        if args:
            items_schema = _to_json_schema(args[0])
            if items_schema is not None:
                schema["items"] = items_schema
        return schema

    # Handle dict[str, X]
    if origin is dict:
        args = get_args(annotation)
        schema_dict: dict[str, Any] = {"type": "object"}
        if args and len(args) >= 2:
            value_schema = _to_json_schema(args[1])
            if value_schema is not None:
                schema_dict["additionalProperties"] = value_schema
        return schema_dict

    # Handle Pydantic BaseModel — use its built-in JSON Schema generation
    if _is_pydantic_model(annotation):
        model_schema: dict[str, Any] = annotation.model_json_schema()
        return model_schema

    return None


def python_type_to_format(annotation: Any) -> dict[str, Any] | None:
    """Convert a Python type annotation to a JSON Schema dict.

    Args:
        annotation: The Python type annotation.

    Returns:
        A JSON Schema dict (with ``$schema`` header) or None if the type is not supported.
    """
    schema = _to_json_schema(annotation)
    if schema is not None:
        schema["$schema"] = _JSON_SCHEMA_DRAFT
    return schema


def extract_request_format(func: Any) -> dict[str, Any] | None:
    """Extract request format from the first parameter of a callable's type hints.

    Args:
        func: A callable (function or method).

    Returns:
        A JSON Schema dict or None if no type hint is available.
    """
    if not callable(func):
        return None

    try:
        hints = get_type_hints(func)
    except Exception:
        return None

    sig = inspect.signature(func)
    params = list(sig.parameters.values())
    if not params:
        return None

    first_param = params[0]
    annotation = hints.get(first_param.name, inspect.Parameter.empty)
    if annotation is inspect.Parameter.empty:
        return None

    return python_type_to_format(annotation)


def extract_response_format(func: Any) -> dict[str, Any] | None:
    """Extract response format from a callable's return type hint.

    Args:
        func: A callable (function or method).

    Returns:
        A JSON Schema dict or None if no return type hint is available.
    """
    if not callable(func):
        return None

    try:
        hints = get_type_hints(func)
    except Exception:
        return None

    return_type = hints.get("return", inspect.Parameter.empty)
    if return_type is inspect.Parameter.empty:
        return None

    return python_type_to_format(return_type)
