import json
import os
from pathlib import Path
from typing import Any

import pytest
from iii_helpers.observability import (
    REDACTED_PLACEHOLDER,
    redact,
    redact_and_truncate,
    resolve_max_bytes_from_env,
)

# Shared vectors the engine and every SDK run (sdk/fixtures/redaction).
URL_VECTORS = Path(__file__).resolve().parents[4] / "fixtures" / "redaction" / "url-credentials.json"
URL_CASES: list[dict[str, str]] = json.loads(URL_VECTORS.read_text(encoding="utf-8"))["cases"]


def test_redacts_top_level_sensitive_keys() -> None:
    out = redact({"api_key": "sk-abc123", "model": "claude-3-5"})
    assert out["api_key"] == REDACTED_PLACEHOLDER
    assert out["model"] == "claude-3-5"


def test_redacts_nested_sensitive_keys() -> None:
    out = redact(
        {
            "headers": {"Authorization": "Bearer xyz", "Content-Type": "application/json"},
            "config": {"secret": "hush"},
        }
    )
    assert out["headers"]["Authorization"] == REDACTED_PLACEHOLDER
    assert out["headers"]["Content-Type"] == "application/json"
    assert out["config"]["secret"] == REDACTED_PLACEHOLDER


def test_walks_into_arrays() -> None:
    out = redact(
        {
            "accounts": [
                {"access_token": "a", "user": "alice"},
                {"access_token": "b", "user": "bob"},
            ]
        }
    )
    assert out["accounts"][0]["access_token"] == REDACTED_PLACEHOLDER
    assert out["accounts"][0]["user"] == "alice"
    assert out["accounts"][1]["access_token"] == REDACTED_PLACEHOLDER


def test_sensitive_parent_key_redacts_entire_subtree() -> None:
    out = redact({"credentials": [{"user": "alice", "token": "a"}]})
    assert out["credentials"] == REDACTED_PLACEHOLDER


def test_case_insensitive_match() -> None:
    out = redact({"API_KEY": "x", "PassWord": "y", "client_SECRET": "z"})
    assert out["API_KEY"] == REDACTED_PLACEHOLDER
    assert out["PassWord"] == REDACTED_PLACEHOLDER
    assert out["client_SECRET"] == REDACTED_PLACEHOLDER


def test_token_alone_matched_but_not_substring() -> None:
    out = redact(
        {
            "token": "tok-1",
            "id_token": "tok-2",
            "notification": "ping",
            "function_id": "do_thing",
        }
    )
    assert out["token"] == REDACTED_PLACEHOLDER
    assert out["id_token"] == REDACTED_PLACEHOLDER
    assert out["notification"] == "ping"
    assert out["function_id"] == "do_thing"


def test_redact_handles_none_and_primitives() -> None:
    assert redact(None) is None
    assert redact(42) == 42
    assert redact("hello") == "hello"
    assert redact(True) is True


def test_redact_handles_tuples() -> None:
    out = redact({"items": ({"token": "t", "user": "u"},)})
    assert isinstance(out["items"], tuple)
    assert out["items"][0]["token"] == REDACTED_PLACEHOLDER
    assert out["items"][0]["user"] == "u"


def test_redact_and_truncate_no_truncation_under_limit() -> None:
    json_str, truncated = redact_and_truncate({"model": "claude-3-5"}, 4096)
    assert truncated is False
    assert "[TRUNCATED]" not in json_str


def test_redact_and_truncate_truncates_over_limit() -> None:
    big = "x" * 8192
    json_str, truncated = redact_and_truncate({"blob": big}, 4096)
    assert truncated is True
    assert "[TRUNCATED]" in json_str
    assert len(json_str.encode("utf-8")) <= 4096


def test_redact_and_truncate_respects_max_below_marker_length() -> None:
    # Marker is ~16 bytes; output must never exceed max_bytes even when
    # the cap is below marker length.
    big = "x" * 100
    for max_bytes in (1, 4, 8, 12):
        json_str, truncated = redact_and_truncate({"blob": big}, max_bytes)
        assert truncated is True
        assert len(json_str.encode("utf-8")) <= max_bytes


def test_redact_and_truncate_no_cap_by_default() -> None:
    big = "x" * 1_000_000
    json_str, truncated = redact_and_truncate({"blob": big})
    assert truncated is False
    assert "[TRUNCATED]" not in json_str
    assert len(json_str.encode("utf-8")) > 1_000_000


def test_redact_and_truncate_zero_means_unlimited() -> None:
    big = "x" * 8192
    json_str, truncated = redact_and_truncate({"blob": big}, 0)
    assert truncated is False
    assert "[TRUNCATED]" not in json_str


def test_redaction_runs_before_truncation() -> None:
    json_str, _ = redact_and_truncate(
        {"api_key": "sk-must-not-leak", "blob": "x" * 8192},
        4096,
    )
    assert "sk-must-not-leak" not in json_str
    assert "[REDACTED]" in json_str


def test_truncation_preserves_utf8_boundaries() -> None:
    s = "aéaéaéaé" * 2000
    json_str, truncated = redact_and_truncate({"v": s}, 100)
    assert truncated is True
    json_str.encode("utf-8")


def test_resolve_max_bytes_unset_returns_none() -> None:
    os.environ.pop("III_TRACE_PAYLOAD_MAX_BYTES", None)
    assert resolve_max_bytes_from_env() is None


def test_resolve_max_bytes_zero_returns_none() -> None:
    os.environ["III_TRACE_PAYLOAD_MAX_BYTES"] = "0"
    try:
        assert resolve_max_bytes_from_env() is None
    finally:
        os.environ.pop("III_TRACE_PAYLOAD_MAX_BYTES", None)


def test_resolve_max_bytes_unlimited_returns_none() -> None:
    os.environ["III_TRACE_PAYLOAD_MAX_BYTES"] = "unlimited"
    try:
        assert resolve_max_bytes_from_env() is None
    finally:
        os.environ.pop("III_TRACE_PAYLOAD_MAX_BYTES", None)


def test_resolve_max_bytes_positive_returns_int() -> None:
    os.environ["III_TRACE_PAYLOAD_MAX_BYTES"] = "8192"
    try:
        assert resolve_max_bytes_from_env() == 8192
    finally:
        os.environ.pop("III_TRACE_PAYLOAD_MAX_BYTES", None)


def test_resolve_max_bytes_garbage_returns_none() -> None:
    os.environ["III_TRACE_PAYLOAD_MAX_BYTES"] = "not-a-number"
    try:
        assert resolve_max_bytes_from_env() is None
    finally:
        os.environ.pop("III_TRACE_PAYLOAD_MAX_BYTES", None)


def _wrap(value: str) -> dict[str, Any]:
    return {"headers": {"location": value}, "links": [value]}


def test_loads_the_shared_url_vectors() -> None:
    assert len(URL_CASES) > 10


@pytest.mark.parametrize("case", URL_CASES, ids=[case["note"] for case in URL_CASES])
def test_redacts_url_credentials(case: dict[str, str]) -> None:
    assert redact(_wrap(case["value"])) == _wrap(case["expect"])
    assert redact(_wrap(case["expect"])) == _wrap(case["expect"])


def test_redacts_the_location_of_a_redirect_to_a_presigned_url() -> None:
    response = {
        "status_code": 302,
        "headers": {
            "location": "https://acct.r2.cloudflarestorage.com/bucket/key?X-Amz-Algorithm=AWS4-HMAC-SHA256"
            "&X-Amz-Credential=AK%2F20261126&X-Amz-Signature=abc"
        },
        "body": {"upload": {"url": "http://localhost:49200/v1/objects/upload?token=eyJ.a.b"}},
    }
    out, _ = redact_and_truncate(response)
    assert out == (
        '{"status_code": 302, "headers": {"location": "https://acct.r2.cloudflarestorage.com/bucket/key'
        '?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=[REDACTED]&X-Amz-Signature=[REDACTED]"}, '
        '"body": {"upload": {"url": "http://localhost:49200/v1/objects/upload?token=[REDACTED]"}}}'
    )
    assert redact(json.loads(out)) == json.loads(out)


def test_reads_a_payload_that_is_a_url_and_leaves_other_strings_alone() -> None:
    assert redact("/v1/objects/download?token=abc") == "/v1/objects/download?token=[REDACTED]"
    assert redact(("https://h.example/?sig=a",)) == ("https://h.example/?sig=[REDACTED]",)
    assert redact("see https://h.example/?token=abc") == "see https://h.example/?token=abc"
    assert redact("ana@example.com") == "ana@example.com"
    assert redact({"https://h.example/?token=abc": 1}) == {"https://h.example/?token=abc": 1}


def test_replaces_a_url_under_a_sensitive_key_whole() -> None:
    assert redact({"token": "https://h.example/?sig=a"}) == {"token": REDACTED_PLACEHOLDER}
