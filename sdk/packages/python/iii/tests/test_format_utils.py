"""Tests for format_utils: type hint to JSON Schema extraction."""

from __future__ import annotations

from typing import Optional

from pydantic import BaseModel, Field

from iii.format_utils import _JSON_SCHEMA_DRAFT, extract_request_format, extract_response_format, python_type_to_format

# ---------------------------------------------------------------------------
# python_type_to_format — primitives
# ---------------------------------------------------------------------------


def test_string_type() -> None:
    fmt = python_type_to_format(str)
    assert fmt == {"type": "string", "$schema": _JSON_SCHEMA_DRAFT}


def test_int_type() -> None:
    fmt = python_type_to_format(int)
    assert fmt == {"type": "integer", "$schema": _JSON_SCHEMA_DRAFT}


def test_float_type() -> None:
    fmt = python_type_to_format(float)
    assert fmt == {"type": "number", "$schema": _JSON_SCHEMA_DRAFT}


def test_bool_type() -> None:
    fmt = python_type_to_format(bool)
    assert fmt == {"type": "boolean", "$schema": _JSON_SCHEMA_DRAFT}


def test_none_type() -> None:
    fmt = python_type_to_format(type(None))
    assert fmt == {"type": "null", "$schema": _JSON_SCHEMA_DRAFT}


# ---------------------------------------------------------------------------
# python_type_to_format — containers
# ---------------------------------------------------------------------------


def test_list_of_str() -> None:
    fmt = python_type_to_format(list[str])
    assert fmt is not None
    assert fmt["$schema"] == _JSON_SCHEMA_DRAFT
    assert fmt["type"] == "array"
    assert fmt["items"] == {"type": "string"}


def test_list_of_int() -> None:
    fmt = python_type_to_format(list[int])
    assert fmt is not None
    assert fmt["type"] == "array"
    assert fmt["items"] == {"type": "integer"}


def test_dict_str_str() -> None:
    fmt = python_type_to_format(dict[str, str])
    assert fmt == {"type": "object", "additionalProperties": {"type": "string"}, "$schema": _JSON_SCHEMA_DRAFT}


def test_dict_str_any() -> None:
    from typing import Any

    fmt = python_type_to_format(dict[str, Any])
    assert fmt == {"type": "object", "$schema": _JSON_SCHEMA_DRAFT}


# ---------------------------------------------------------------------------
# python_type_to_format — Optional
# ---------------------------------------------------------------------------


def test_optional_str() -> None:
    fmt = python_type_to_format(Optional[str])
    assert fmt is not None
    assert fmt["type"] == ["string", "null"]
    assert fmt["$schema"] == _JSON_SCHEMA_DRAFT


def test_optional_int_pipe_syntax() -> None:
    fmt = python_type_to_format(int | None)
    assert fmt is not None
    assert fmt["type"] == ["integer", "null"]
    assert fmt["$schema"] == _JSON_SCHEMA_DRAFT


# ---------------------------------------------------------------------------
# python_type_to_format — Pydantic BaseModel (JSON Schema)
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
    fmt = python_type_to_format(Address)
    assert fmt is not None
    assert fmt["$schema"] == _JSON_SCHEMA_DRAFT
    assert fmt["type"] == "object"
    assert "street" in fmt["properties"]
    assert "city" in fmt["properties"]
    assert "zip_code" in fmt["properties"]
    assert "street" in fmt["required"]
    assert "city" in fmt["required"]
    assert "zip_code" not in fmt.get("required", [])


def test_nested_model() -> None:
    fmt = python_type_to_format(Person)
    assert fmt is not None
    assert fmt["$schema"] == _JSON_SCHEMA_DRAFT
    assert fmt["type"] == "object"
    props = fmt["properties"]
    assert "name" in props
    assert "age" in props
    assert "address" in props
    assert "tags" in props
    age_prop = props["age"]
    assert age_prop.get("description") == "Age in years"
    # Nested model should use $defs and $ref (Pydantic v2 / Draft 2020-12)
    assert "$defs" in fmt
    assert "Address" in fmt["$defs"]
    assert props["address"]["$ref"].startswith("#/$defs/")


def test_list_of_model() -> None:
    fmt = python_type_to_format(list[Address])
    assert fmt is not None
    assert fmt["$schema"] == _JSON_SCHEMA_DRAFT
    assert fmt["type"] == "array"
    items = fmt["items"]
    assert items["type"] == "object"
    assert "street" in items["properties"]


# ---------------------------------------------------------------------------
# python_type_to_format — unsupported types
# ---------------------------------------------------------------------------


def test_unsupported_type_returns_none() -> None:
    from typing import Any

    assert python_type_to_format(Any) is None


def test_no_annotation_returns_none() -> None:
    import inspect

    assert python_type_to_format(inspect.Parameter.empty) is None


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
    assert fmt["$schema"] == _JSON_SCHEMA_DRAFT
    assert fmt["type"] == "object"
    assert "name" in fmt["properties"]
    assert "greeting" in fmt["properties"]
    assert "name" in fmt["required"]
    assert "greeting" not in fmt.get("required", [])


def test_extract_request_from_primitive_param() -> None:
    async def handler(data: str) -> str:
        return data.upper()

    fmt = extract_request_format(handler)
    assert fmt == {"type": "string", "$schema": _JSON_SCHEMA_DRAFT}


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
    assert fmt["$schema"] == _JSON_SCHEMA_DRAFT
    assert fmt["type"] == "object"
    assert "message" in fmt["properties"]
    assert "message" in fmt["required"]
    assert fmt["title"] == "GreetOutput"


def test_extract_response_primitive() -> None:
    async def handler(data: str) -> str:
        return data

    fmt = extract_response_format(handler)
    assert fmt == {"type": "string", "$schema": _JSON_SCHEMA_DRAFT}


def test_extract_response_none_return() -> None:
    async def handler(data: str) -> None:
        pass

    fmt = extract_response_format(handler)
    assert fmt == {"type": "null", "$schema": _JSON_SCHEMA_DRAFT}


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
    assert req["$schema"] == _JSON_SCHEMA_DRAFT
    assert req["type"] == "object"
    assert "name" in req["properties"]
    assert res is not None
    assert res["$schema"] == _JSON_SCHEMA_DRAFT
    assert res["type"] == "object"
    assert res["title"] == "GreetOutput"


# ---------------------------------------------------------------------------
# JSON Schema structure validation
# ---------------------------------------------------------------------------


def test_pydantic_model_produces_json_schema_with_title() -> None:
    fmt = python_type_to_format(GreetInput)
    assert fmt is not None
    assert fmt["$schema"] == _JSON_SCHEMA_DRAFT
    assert fmt["title"] == "GreetInput"
    assert fmt["type"] == "object"
    assert "properties" in fmt
    assert "required" in fmt


def test_pydantic_model_required_list() -> None:
    """required should be a list of field names, not a bool."""
    fmt = python_type_to_format(Address)
    assert fmt is not None
    assert isinstance(fmt["required"], list)
    assert set(fmt["required"]) == {"street", "city"}
