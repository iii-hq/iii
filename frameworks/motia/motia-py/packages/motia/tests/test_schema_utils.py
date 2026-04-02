from pydantic import BaseModel

from motia.schema_utils import schema_to_json_schema


class ExampleModel(BaseModel):
    name: str


def test_schema_to_json_schema_handles_supported_inputs() -> None:
    schema = {"type": "object"}

    assert schema_to_json_schema(None) is None
    assert schema_to_json_schema(schema) is schema

    model_schema = schema_to_json_schema(ExampleModel)
    assert model_schema is not None
    assert model_schema["properties"]["name"]["title"] == "Name"


def test_schema_to_json_schema_returns_none_for_unsupported_values() -> None:
    assert schema_to_json_schema("unsupported") is None

