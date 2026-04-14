from __future__ import annotations

import pytest

from iii import InitOptions
from iii.iii import III


def _call_metadata_method() -> dict[str, object]:
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
