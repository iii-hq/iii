"""Tests for _get_worker_metadata's isolation field.

Existing tests mock ``_register_worker_metadata`` wholesale, so the new
``isolation`` key on ``_get_worker_metadata``'s return dict has no other
coverage. These tests exercise the env-var read path directly by calling the
method on a minimal stub that bypasses ``III.__init__`` (which starts threads
and background tasks we don't need for a metadata unit test).
"""

from __future__ import annotations

import pytest

from iii import InitOptions
from iii.iii import III


def _call_metadata_method() -> dict[str, object]:
    """Call III._get_worker_metadata on a minimal stub, bypassing __init__."""
    stub = III.__new__(III)
    stub._options = InitOptions()
    return stub._get_worker_metadata()


def test_get_worker_metadata_isolation_is_none_when_env_unset(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.delenv("III_ISOLATION", raising=False)

    metadata = _call_metadata_method()

    assert "isolation" in metadata
    assert metadata["isolation"] is None


def test_get_worker_metadata_forwards_iii_isolation_env_var(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("III_ISOLATION", "docker")

    metadata = _call_metadata_method()

    assert metadata["isolation"] == "docker"
