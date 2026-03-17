"""Tests for iii.utils."""

import pytest

from iii.utils import safe_stringify


def test_safe_stringify_dict() -> None:
    assert safe_stringify({"a": 1, "b": "x"}) == '{"a": 1, "b": "x"}'


def test_safe_stringify_list() -> None:
    assert safe_stringify([1, 2, 3]) == "[1, 2, 3]"


def test_safe_stringify_none() -> None:
    assert safe_stringify(None) == "null"


def test_safe_stringify_uses_default_str_for_non_serializable() -> None:
    class Custom:
        def __str__(self) -> str:
            return "custom-repr"

    assert safe_stringify({"obj": Custom()}) == '{"obj": "custom-repr"}'


def test_safe_stringify_circular_reference() -> None:
    circular: dict[str, object] = {}
    circular["self"] = circular
    result = safe_stringify(circular)
    assert result == "[unserializable]"


def test_safe_stringify_type_error_returns_unserializable() -> None:
    class BadRepr:
        def __str__(self) -> str:
            raise TypeError("cannot stringify")

    result = safe_stringify(BadRepr())
    assert result == "[unserializable]"


def test_safe_stringify_value_error_returns_unserializable() -> None:
    class BadValue:
        def __str__(self) -> str:
            raise ValueError("bad value")

    result = safe_stringify(BadValue())
    assert result == "[unserializable]"
