"""Payload redaction + truncation for invocation event capture."""

from __future__ import annotations

import json
import os
from typing import Any, Optional

REDACTED_PLACEHOLDER = "[REDACTED]"
_TRUNCATION_MARKER = '..."[TRUNCATED]"'


def resolve_max_bytes_from_env() -> Optional[int]:
    raw = os.environ.get("III_TRACE_PAYLOAD_MAX_BYTES")
    if raw is None:
        return None
    trimmed = raw.strip()
    if not trimmed or trimmed.lower() == "unlimited":
        return None
    try:
        parsed = int(trimmed)
    except ValueError:
        return None
    if parsed <= 0:
        return None
    return parsed

_SENSITIVE_FRAGMENTS = (
    "api_key",
    "apikey",
    "api-key",
    "password",
    "secret",
    "credential",
    "authorization",
    "auth_token",
    "access_token",
    "refresh_token",
    "bearer",
    "private_key",
    "client_secret",
)


def _is_sensitive_key(key: str) -> bool:
    lower = key.lower()
    if any(fragment in lower for fragment in _SENSITIVE_FRAGMENTS):
        return True
    # ``token`` alone is too common a substring; require whole-key or suffix match.
    return lower == "token" or lower.endswith("_token") or lower.endswith("-token")


def redact(value: Any) -> Any:
    if isinstance(value, dict):
        return {
            k: REDACTED_PLACEHOLDER if _is_sensitive_key(k) else redact(v)
            for k, v in value.items()
        }
    if isinstance(value, list):
        return [redact(item) for item in value]
    if isinstance(value, tuple):
        return tuple(redact(item) for item in value)
    return value


def redact_and_truncate(
    value: Any, max_bytes: Optional[int] = None
) -> tuple[str, bool]:
    redacted = redact(value)
    try:
        serialized = json.dumps(redacted, default=str, ensure_ascii=False)
    except (TypeError, ValueError):
        serialized = "null"

    if max_bytes is None or max_bytes <= 0:
        return serialized, False

    encoded = serialized.encode("utf-8")
    if len(encoded) <= max_bytes:
        return serialized, False

    marker_len = len(_TRUNCATION_MARKER.encode("utf-8"))
    cap = max(0, max_bytes - marker_len)
    # Walk back to a UTF-8 boundary so we don't emit half-codepoints.
    cut = cap
    while cut > 0 and (encoded[cut] & 0xC0) == 0x80:
        cut -= 1
    truncated = encoded[:cut].decode("utf-8", errors="ignore") + _TRUNCATION_MARKER
    return truncated, True
