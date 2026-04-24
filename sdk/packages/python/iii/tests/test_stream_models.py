"""Unit tests for stream model serialization."""

from iii.stream import UpdateAppend


def test_update_append_model_serializes() -> None:
    op = UpdateAppend(path="chunks", value={"text": "hello"})

    assert op.model_dump() == {"type": "append", "path": "chunks", "value": {"text": "hello"}}
