from types import SimpleNamespace

from pydantic import BaseModel

from motia.triggers import queue
from motia.types import StepConfig
from motia.validator import (
    check_schema_compatibility,
    pydantic_to_json_schema,
    validate_schema,
    validate_step,
    validate_step_config,
)


class PayloadModel(BaseModel):
    name: str


def test_validate_schema_rejects_invalid_shapes() -> None:
    assert validate_schema("bad") == ["Schema must be a dictionary"]
    assert validate_schema({}) == ["Schema should have a 'type', '$ref', 'anyOf', or 'oneOf' property"]
    assert validate_schema({"type": "object", "properties": []}) == ["'properties' must be a dictionary"]
    assert validate_schema({"type": "object", "required": "name"}) == ["'required' must be a list"]
    assert validate_schema({"type": "array", "items": []}) == ["'items' must be a dictionary"]


def test_validate_schema_accepts_valid_minimal_shapes() -> None:
    assert validate_schema({"type": "object", "properties": {"name": {"type": "string"}}}) == []
    assert validate_schema({"$ref": "#/definitions/Model"}) == []


def test_validate_step_config_reports_missing_name_and_triggers() -> None:
    assert validate_step_config(object()) == ["Step config must have a 'name' property"]

    invalid = SimpleNamespace(name="", triggers=[])
    errors = validate_step_config(invalid)

    assert "Step name cannot be empty" in errors
    assert "Step must have at least one trigger" in errors


def test_validate_step_returns_validation_errors_for_invalid_config() -> None:
    errors = validate_step({"name": "missing-triggers"})

    assert errors
    assert "Field required" in errors[0]


def test_validate_step_accepts_valid_step_config() -> None:
    config = StepConfig(name="valid-step", triggers=[queue("topic.created")])

    assert validate_step(config) == []


def test_pydantic_to_json_schema_converts_model() -> None:
    schema = pydantic_to_json_schema(PayloadModel)

    assert schema["properties"]["name"]["title"] == "Name"


def test_check_schema_compatibility_reports_mismatches() -> None:
    type_issues = check_schema_compatibility({"type": "string"}, {"type": "number"})
    assert type_issues == ["Type mismatch: string vs number"]

    issues = check_schema_compatibility(
        {
            "type": "object",
            "properties": {"id": {"type": "string"}},
            "required": ["id"],
        },
        {
            "type": "object",
            "properties": {"other": {"type": "string"}},
            "required": ["other"],
        },
    )

    assert "Required fields missing in second schema" in issues[0]
    assert "Required fields missing in first schema" in issues[1]

