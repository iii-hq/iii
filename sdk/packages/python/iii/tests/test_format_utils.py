"""Tests for format_utils: type hint to RegisterFunctionFormat extraction."""

from __future__ import annotations

from typing import Optional

import pytest
from pydantic import BaseModel, Field

from iii.format_utils import extract_request_format, extract_response_format, python_type_to_format
from iii.iii_types import RegisterFunctionFormat

# ---------------------------------------------------------------------------
# python_type_to_format — primitives
# ---------------------------------------------------------------------------


def test_string_type() -> None:
    fmt = python_type_to_format(str, "name")
    assert fmt is not None
    assert fmt.type == "string"
    assert fmt.name == "name"
    assert fmt.required is True


def test_int_type() -> None:
    fmt = python_type_to_format(int, "age")
    assert fmt is not None
    assert fmt.type == "number"


def test_float_type() -> None:
    fmt = python_type_to_format(float, "score")
    assert fmt is not None
    assert fmt.type == "number"


def test_bool_type() -> None:
    fmt = python_type_to_format(bool, "active")
    assert fmt is not None
    assert fmt.type == "boolean"


def test_none_type() -> None:
    fmt = python_type_to_format(type(None), "nothing")
    assert fmt is not None
    assert fmt.type == "null"


# ---------------------------------------------------------------------------
# python_type_to_format — containers
# ---------------------------------------------------------------------------


def test_list_of_str() -> None:
    fmt = python_type_to_format(list[str], "tags")
    assert fmt is not None
    assert fmt.type == "array"
    assert fmt.items is not None
    assert fmt.items.type == "string"


def test_list_of_int() -> None:
    fmt = python_type_to_format(list[int], "ids")
    assert fmt is not None
    assert fmt.type == "array"
    assert fmt.items is not None
    assert fmt.items.type == "number"


def test_dict_str_any() -> None:
    fmt = python_type_to_format(dict[str, str], "meta")
    assert fmt is not None
    assert fmt.type == "map"


# ---------------------------------------------------------------------------
# python_type_to_format — Optional
# ---------------------------------------------------------------------------


def test_optional_str() -> None:
    fmt = python_type_to_format(Optional[str], "nickname")
    assert fmt is not None
    assert fmt.type == "string"
    assert fmt.required is False


def test_optional_int_pipe_syntax() -> None:
    fmt = python_type_to_format(int | None, "count")
    assert fmt is not None
    assert fmt.type == "number"
    assert fmt.required is False


# ---------------------------------------------------------------------------
# python_type_to_format — Pydantic BaseModel
# ---------------------------------------------------------------------------


class Address(BaseModel):
    street: str
    city: str
    zip_code: str | None = None


class Person(BaseModel):
    name: str
    age: int = Field(description="Age in years")
    address: Address
    tags: list[str] = Field(default_factory=list)


def test_simple_model() -> None:
    fmt = python_type_to_format(Address, "address")
    assert fmt is not None
    assert fmt.type == "object"
    assert fmt.body is not None
    assert len(fmt.body) == 3
    names = {f.name for f in fmt.body}
    assert names == {"street", "city", "zip_code"}
    zip_field = next(f for f in fmt.body if f.name == "zip_code")
    assert zip_field.required is False


def test_nested_model() -> None:
    fmt = python_type_to_format(Person, "person")
    assert fmt is not None
    assert fmt.type == "object"
    assert fmt.body is not None
    names = {f.name for f in fmt.body}
    assert names == {"name", "age", "address", "tags"}
    age_field = next(f for f in fmt.body if f.name == "age")
    assert age_field.description == "Age in years"
    address_field = next(f for f in fmt.body if f.name == "address")
    assert address_field.type == "object"
    assert address_field.body is not None
    assert len(address_field.body) == 3


def test_list_of_model() -> None:
    fmt = python_type_to_format(list[Address], "addresses")
    assert fmt is not None
    assert fmt.type == "array"
    assert fmt.items is not None
    assert fmt.items.type == "object"
    assert fmt.items.body is not None
    assert len(fmt.items.body) == 3


# ---------------------------------------------------------------------------
# python_type_to_format — unsupported types
# ---------------------------------------------------------------------------


def test_unsupported_type_returns_none() -> None:
    from typing import Any

    assert python_type_to_format(Any, "x") is None


def test_no_annotation_returns_none() -> None:
    import inspect

    assert python_type_to_format(inspect.Parameter.empty, "x") is None


# ---------------------------------------------------------------------------
# python_type_to_format — description and required
# ---------------------------------------------------------------------------


def test_description_passed_through() -> None:
    fmt = python_type_to_format(str, "name", description="The user's name")
    assert fmt is not None
    assert fmt.description == "The user's name"


def test_required_default_true() -> None:
    fmt = python_type_to_format(str, "name")
    assert fmt is not None
    assert fmt.required is True


def test_required_explicit_false() -> None:
    fmt = python_type_to_format(str, "name", required=False)
    assert fmt is not None
    assert fmt.required is False


# ---------------------------------------------------------------------------
# extract_request_format
# ---------------------------------------------------------------------------


class GreetInput(BaseModel):
    name: str
    greeting: str = "Hello"


def test_extract_request_from_pydantic_param() -> None:
    async def handler(data: GreetInput) -> str:
        return f"{data.greeting}, {data.name}!"

    fmt = extract_request_format(handler)
    assert fmt is not None
    assert fmt.type == "object"
    assert fmt.body is not None
    names = {f.name for f in fmt.body}
    assert "name" in names
    assert "greeting" in names


def test_extract_request_from_primitive_param() -> None:
    async def handler(data: str) -> str:
        return data.upper()

    fmt = extract_request_format(handler)
    assert fmt is not None
    assert fmt.type == "string"


def test_extract_request_no_annotation() -> None:
    async def handler(data):
        return data

    fmt = extract_request_format(handler)
    assert fmt is None


def test_extract_request_no_params() -> None:
    def noop() -> None:
        pass

    fmt = extract_request_format(noop)
    assert fmt is None


def test_extract_request_not_callable() -> None:
    fmt = extract_request_format("not a function")
    assert fmt is None


# ---------------------------------------------------------------------------
# extract_response_format
# ---------------------------------------------------------------------------


class GreetOutput(BaseModel):
    message: str


def test_extract_response_pydantic() -> None:
    async def handler(data: GreetInput) -> GreetOutput:
        return GreetOutput(message="hi")

    fmt = extract_response_format(handler)
    assert fmt is not None
    assert fmt.type == "object"
    assert fmt.body is not None
    assert len(fmt.body) == 1
    assert fmt.body[0].name == "message"


def test_extract_response_primitive() -> None:
    async def handler(data: str) -> str:
        return data

    fmt = extract_response_format(handler)
    assert fmt is not None
    assert fmt.type == "string"


def test_extract_response_none_return() -> None:
    async def handler(data: str) -> None:
        pass

    fmt = extract_response_format(handler)
    assert fmt is not None
    assert fmt.type == "null"


def test_extract_response_no_return_type() -> None:
    async def handler(data: str):
        return data

    fmt = extract_response_format(handler)
    assert fmt is None


def test_extract_response_not_callable() -> None:
    fmt = extract_response_format(42)
    assert fmt is None


# ---------------------------------------------------------------------------
# Sync handler support
# ---------------------------------------------------------------------------


def test_extract_from_sync_handler() -> None:
    def handler(data: GreetInput) -> GreetOutput:
        return GreetOutput(message="hi")

    req = extract_request_format(handler)
    res = extract_response_format(handler)
    assert req is not None
    assert req.type == "object"
    assert res is not None
    assert res.type == "object"
