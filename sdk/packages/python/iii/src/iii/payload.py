"""Payload redaction + truncation for invocation event capture.

Mirrors `sdk/packages/rust/iii/src/telemetry/payload.rs` and
`sdk/packages/node/iii/src/telemetry-system/payload.ts` so worker chains
crossing language boundaries produce consistently safe events.

Payload capture is ON by default; gate with ``III_DISABLE_TRACE_PAYLOADS=1``.
Two passes run before the payload leaves the process:

1. Recursive redaction of well-known credential keys.
2. OPTIONAL JSON-byte truncation. By default we emit the full payload.
   Set ``III_TRACE_PAYLOAD_MAX_BYTES=N`` to cap each serialized payload
   at N bytes; ``0`` or ``unlimited`` (or unset) means no cap.
"""

from __future__ import annotations

import json
import os
from typing import Any, Optional

REDACTED_PLACEHOLDER = "[REDACTED]"
_TRUNCATION_MARKER = '..."[TRUNCATED]"'


def resolve_max_bytes_from_env() -> Optional[int]:
    """Read ``III_TRACE_PAYLOAD_MAX_BYTES``.

    Returns a positive integer or ``None`` (no cap) for unset / ``"0"`` /
    ``"unlimited"`` / unparseable.
    """
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
    # ``token`` alone is too common as a substring; gate to whole-key or suffix.
    return lower == "token" or lower.endswith("_token") or lower.endswith("-token")


def redact(value: Any) -> Any:
    """Recursively redact values of sensitive keys. Returns a new value."""
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
    """Redact sensitive keys then serialize to JSON, optionally capped at
    ``max_bytes``.

    Pass ``None`` (or unset ``III_TRACE_PAYLOAD_MAX_BYTES``) to emit the
    full payload — the new default.

    Returns ``(json_string, truncated)``. When ``truncated`` is True, the
    returned string ends with the truncation marker so consumers can tell
    the payload was clipped. Truncation operates on UTF-8 byte boundaries.
    """
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
    # Walk back to a valid UTF-8 boundary. A continuation byte has the
    # top two bits set to 10 (0x80-0xBF); slice such that we don't land
    # in the middle of a multi-byte sequence.
    cut = cap
    while cut > 0 and (encoded[cut] & 0xC0) == 0x80:
        cut -= 1
    truncated = encoded[:cut].decode("utf-8", errors="ignore") + _TRUNCATION_MARKER
    return truncated, True
