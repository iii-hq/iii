"""Utility functions for the III SDK."""

from __future__ import annotations

import json
from typing import Any

from .format_utils import extract_request_format, extract_response_format, python_type_to_format
from .types import is_channel_ref as is_channel_ref  # noqa: F401 - re-exported from types

__all__ = [
    "extract_request_format",
    "extract_response_format",
    "is_channel_ref",
    "python_type_to_format",
    "safe_stringify",
]


def safe_stringify(value: Any) -> str:
    """Safely stringify a value, handling circular references and non-serializable types."""
    try:
        return json.dumps(value, default=str)
    except (TypeError, ValueError):
        return "[unserializable]"
