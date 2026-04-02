"""Tests for runtime input schema validation behavior."""

import logging

import pytest
from pydantic import BaseModel, ValidationError

from motia.runtime import _validate_input_schema


class _PydanticPayload(BaseModel):
    count: int


def test_validate_input_schema_raises_pydantic_validation_error(caplog: pytest.LogCaptureFixture) -> None:
    """Pydantic schema validation errors should be logged and re-raised."""
    with caplog.at_level(logging.ERROR, logger="motia.runtime"):
        with pytest.raises(ValidationError):
            _validate_input_schema(_PydanticPayload, {"count": "not-an-int"}, "queue:test")

    assert "queue:test" in caplog.text
    assert "schema=" in caplog.text


def test_validate_input_schema_raises_jsonschema_validation_error(caplog: pytest.LogCaptureFixture) -> None:
    """JSON schema validation errors should be logged and re-raised."""
    jsonschema = pytest.importorskip("jsonschema")

    schema = {
        "type": "object",
        "properties": {"count": {"type": "integer"}},
        "required": ["count"],
    }

    with caplog.at_level(logging.ERROR, logger="motia.runtime"):
        with pytest.raises(jsonschema.ValidationError):
            _validate_input_schema(schema, {"count": "not-an-int"}, "api:test")

    assert "api:test" in caplog.text
    assert "json_schema=" in caplog.text
