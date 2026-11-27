"""Payload redaction + truncation for invocation event capture."""

from __future__ import annotations

import json
import os
import re
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


# Query parameters that carry a credential in a signed URL (S3 and R2 SigV2/SigV4, GCS,
# Azure SAS, CloudFront, the storage worker's local provider), lowercased. The engine and
# every SDK carry the same list.
_SENSITIVE_URL_PARAMS = frozenset(
    (
        "token",
        "sig",
        "signature",
        "policy",
        "key-pair-id",
        "awsaccesskeyid",
        "x-amz-signature",
        "x-amz-credential",
        "x-amz-security-token",
        "x-goog-signature",
        "x-goog-credential",
    )
)

_URL_SCHEME = re.compile(r"[A-Za-z][A-Za-z0-9+.-]*://")


def _is_sensitive_url_param(name: str) -> bool:
    return _is_sensitive_key(name) or name.lower() in _SENSITIVE_URL_PARAMS


def _redact_url(text: str) -> str:
    """Replace the credentials in ``text`` with the placeholder when it is a URL.

    A URL starts with ``scheme://`` or ``/``. The password of its userinfo goes, or the
    whole userinfo when it has no ``:`` (a token used as the user name); the authority runs
    to the first ``/``, ``?`` or ``#``, the userinfo to the last ``@`` in it. So does the
    value of every query parameter whose name is sensitive (the query runs from the first
    ``?`` to the next ``#``). Everything else is kept as it is; empty
    and already redacted values are skipped.
    """
    if text.startswith("/"):
        authority_start = -1
    else:
        scheme = _URL_SCHEME.match(text)
        if scheme is None:
            return text
        authority_start = scheme.end()
    secrets: list[tuple[int, int]] = []

    def secret(start: int, end: int) -> None:
        value = text[start:end]
        if value and value != REDACTED_PLACEHOLDER:
            secrets.append((start, end))

    if authority_start != -1:
        authority_end = len(text)
        for i in range(authority_start, len(text)):
            if text[i] in "/?#":
                authority_end = i
                break
        at = text.rfind("@", authority_start, authority_end)
        if at != -1:
            colon = text.find(":", authority_start, at)
            secret(colon + 1 if colon != -1 else authority_start, at)
    question = text.find("?")
    if question != -1:
        query_end = text.find("#", question + 1)
        if query_end == -1:
            query_end = len(text)
        param_start = question + 1
        for param in text[question + 1 : query_end].split("&"):
            name, equals, _ = param.partition("=")
            if equals and _is_sensitive_url_param(name):
                secret(param_start + len(name) + 1, param_start + len(param))
            param_start += len(param) + 1
    if not secrets:
        return text
    out: list[str] = []
    copied = 0
    for start, end in secrets:
        out.append(text[copied:start])
        out.append(REDACTED_PLACEHOLDER)
        copied = end
    out.append(text[copied:])
    return "".join(out)


def redact(value: Any) -> Any:
    """Recursively redact values of sensitive keys. Returns a new value.

    String values that are URLs lose their credentials too: the password of the
    userinfo (the whole userinfo when it has no password) and the values of query
    parameters with a sensitive name, such as the signature of a presigned URL.

    Args:
        value: Value to redact; dicts, lists, and tuples are traversed recursively.
    """
    if isinstance(value, str):
        return _redact_url(value)
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
    """Redact then serialize to JSON, optionally capped at ``max_bytes``.

    Args:
        value: Value to redact and serialize.
        max_bytes: Optional cap on the serialized byte length; unset, zero, or negative means no cap.
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
    if max_bytes <= marker_len:
        return _TRUNCATION_MARKER[:max_bytes], True

    cap = max_bytes - marker_len
    # Walk back to a UTF-8 boundary so we don't emit half-codepoints.
    cut = cap
    while cut > 0 and (encoded[cut] & 0xC0) == 0x80:
        cut -= 1
    truncated = encoded[:cut].decode("utf-8", errors="ignore") + _TRUNCATION_MARKER
    return truncated, True
