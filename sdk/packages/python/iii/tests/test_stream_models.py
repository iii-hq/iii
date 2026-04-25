"""Unit tests for stream model serialization."""

import json

from iii.stream import StreamUpdateResult, UpdateAppend, UpdateMerge, UpdateOpError


def test_update_append_model_serializes() -> None:
    op = UpdateAppend(path="chunks", value={"text": "hello"})

    assert op.model_dump() == {"type": "append", "path": "chunks", "value": {"text": "hello"}}


def test_update_merge_with_string_path_round_trips() -> None:
    op = UpdateMerge(path="session-abc", value={"author": "alice"})
    dumped = op.model_dump()
    assert dumped == {
        "type": "merge",
        "path": "session-abc",
        "value": {"author": "alice"},
    }
    # JSON round-trip preserves the string form.
    parsed = UpdateMerge.model_validate(json.loads(json.dumps(dumped)))
    assert parsed.path == "session-abc"


def test_update_merge_with_array_path_round_trips() -> None:
    op = UpdateMerge(path=["sessions", "abc"], value={"ts": "chunk"})
    dumped = op.model_dump()
    assert dumped == {
        "type": "merge",
        "path": ["sessions", "abc"],
        "value": {"ts": "chunk"},
    }
    parsed = UpdateMerge.model_validate(json.loads(json.dumps(dumped)))
    assert parsed.path == ["sessions", "abc"]


def test_update_merge_without_path_round_trips() -> None:
    op = UpdateMerge(value={"x": 1})
    dumped = op.model_dump()
    assert dumped == {"type": "merge", "path": None, "value": {"x": 1}}
    parsed = UpdateMerge.model_validate(json.loads(json.dumps(dumped)))
    assert parsed.path is None


def test_update_op_error_round_trip() -> None:
    err = UpdateOpError(
        op_index=0,
        code="merge.path.too_deep",
        message="Path depth 33 exceeds maximum of 32",
        doc_url="https://docs.iii.dev/workers/iii-state#merge-bounds",
    )
    dumped = err.model_dump()
    assert dumped["code"] == "merge.path.too_deep"
    assert dumped["op_index"] == 0


def test_stream_update_result_with_errors_round_trips() -> None:
    result = StreamUpdateResult[dict](
        old_value=None,
        new_value={"a": 1},
        errors=[
            UpdateOpError(
                op_index=0,
                code="merge.path.proto_polluted",
                message='Path segment "__proto__" is a prototype-pollution sink',
            )
        ],
    )
    dumped = result.model_dump()
    assert len(dumped["errors"]) == 1
    assert dumped["errors"][0]["code"] == "merge.path.proto_polluted"
