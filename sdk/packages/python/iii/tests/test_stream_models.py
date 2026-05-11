"""Unit tests for stream model serialization."""

import json

from iii.stream import StreamUpdateResult, UpdateAppend, UpdateMerge, UpdateOpError


def test_update_append_model_serializes() -> None:
    op = UpdateAppend(path="chunks", value={"text": "hello"})

    assert op.model_dump() == {"type": "append", "path": "chunks", "value": {"text": "hello"}}


def test_update_append_with_array_path_round_trips() -> None:
    """Closes issue #1552 case 3: nested-path array form is the new happy path."""
    op = UpdateAppend(path=["entityId", "buffer"], value="chunk")
    dumped = op.model_dump()
    assert dumped == {
        "type": "append",
        "path": ["entityId", "buffer"],
        "value": "chunk",
    }
    parsed = UpdateAppend.model_validate(json.loads(json.dumps(dumped)))
    assert parsed.path == ["entityId", "buffer"]


def test_update_append_without_path_round_trips() -> None:
    """Root append omits ``path`` from the wire (parity with the Rust
    ``skip_serializing_if = "Option::is_none"`` on
    ``UpdateOp::Append.path`` — surfaced in #1612 review)."""
    op = UpdateAppend(value="first")
    dumped = op.model_dump()
    assert dumped == {"type": "append", "value": "first"}
    assert "path" not in dumped
    parsed = UpdateAppend.model_validate(json.loads(json.dumps(dumped)))
    assert parsed.path is None


def test_update_append_with_explicit_none_path() -> None:
    """Explicit ``None`` and missing field both round-trip through a
    wire payload that has no ``path`` key. ``path: null`` payloads
    coming from older clients still deserialize for backward compat."""
    op = UpdateAppend(path=None, value=42)
    assert op.path is None
    assert op.model_dump() == {"type": "append", "value": 42}
    parsed = UpdateAppend.model_validate({"type": "append", "value": 42})
    assert parsed.path is None
    parsed_null = UpdateAppend.model_validate(
        {"type": "append", "path": None, "value": 42}
    )
    assert parsed_null.path is None


def test_update_append_with_empty_string_path() -> None:
    """Empty string is preserved as `Single("")` — engine maps it to root append."""
    op = UpdateAppend(path="", value="x")
    dumped = op.model_dump()
    assert dumped["path"] == ""


def test_update_append_with_dotted_string_keeps_literal_segment() -> None:
    """`"a.b"` is a single literal key, never traversed as `a -> b`."""
    op = UpdateAppend(path="entityId.buffer", value="literal")
    parsed = UpdateAppend.model_validate(json.loads(json.dumps(op.model_dump())))
    assert parsed.path == "entityId.buffer"


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
    """Same wire-format rule as ``UpdateAppend``: root merge omits the
    ``path`` key entirely. ``path: null`` payloads still deserialize."""
    op = UpdateMerge(value={"x": 1})
    dumped = op.model_dump()
    assert dumped == {"type": "merge", "value": {"x": 1}}
    assert "path" not in dumped
    parsed = UpdateMerge.model_validate(json.loads(json.dumps(dumped)))
    assert parsed.path is None
    parsed_null = UpdateMerge.model_validate(
        {"type": "merge", "path": None, "value": {"x": 1}}
    )
    assert parsed_null.path is None


def test_update_op_error_round_trip() -> None:
    err = UpdateOpError(
        op_index=0,
        code="merge.path.too_deep",
        message="Path depth 33 exceeds maximum of 32",
        doc_url="https://iii.dev/docs/workers/iii-state#merge-bounds",
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
