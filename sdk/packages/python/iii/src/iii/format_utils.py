"""Utilities for extracting RegisterFunctionFormat from Python type hints."""

from __future__ import annotations

import inspect
import types
import typing
from typing import Any, get_args, get_origin, get_type_hints

from .iii_types import RegisterFunctionFormat

_PRIMITIVE_MAP: dict[type, str] = {
    str: "string",
    int: "number",
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


def python_type_to_format(
    annotation: Any,
    name: str,
    required: bool = True,
    origin = get_origin(annotation)
    if origin is list:
        args = get_args(annotation)
        items = python_type_to_format(args[0], "item") if args else None
        return RegisterFunctionFormat(
            name=name, type="array", required=required, description=description, items=items
        )

    # Handle dict[str, X]
    if origin is dict:
        return RegisterFunctionFormat(name=name, type="map", required=required, description=description)

    # Handle Pydantic BaseModel
    if _is_pydantic_model(annotation):
        body = _model_fields_to_formats(annotation)
        return RegisterFunctionFormat(
            name=name, type="object", required=required, description=description, body=body or None
        )

    return None


def _model_fields_to_formats(model_cls: type) -> list[RegisterFunctionFormat]:
    """Extract RegisterFunctionFormat list from a Pydantic BaseModel's fields."""
    formats: list[RegisterFunctionFormat] = []
    for field_name, field_info in model_cls.model_fields.items():
        fmt = python_type_to_format(
            field_info.annotation,
            name=field_name,
            required=field_info.is_required(),
            description=field_info.description,
        )
        if fmt is not None:
            formats.append(fmt)
    return formats


def extract_request_format(func: Any) -> RegisterFunctionFormat | None:
    """Extract request format from the first parameter of a callable's type hints.

    Args:
        func: A callable (function or method).

    Returns:
        A RegisterFunctionFormat or None if no type hint is available.
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

    return python_type_to_format(annotation, name=first_param.name)


def extract_response_format(func: Any) -> RegisterFunctionFormat | None:
    """Extract response format from a callable's return type hint.

    Args:
        func: A callable (function or method).

    Returns:
        A RegisterFunctionFormat or None if no return type hint is available.
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

    return python_type_to_format(return_type, name="return")
