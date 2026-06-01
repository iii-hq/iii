from __future__ import annotations

import json
import os
import sys
from pathlib import Path

import pytest

from iii import InitOptions
from iii.iii import III, _detect_project_name
from iii.iii_constants import TelemetryOptions

# pyproject.toml parsing relies on stdlib tomllib (Python 3.11+).
# On 3.10 the SDK intentionally falls back to the cwd basename, so
# tests that assert the parsed name are skipped on that runtime.
requires_tomllib = pytest.mark.skipif(
    sys.version_info < (3, 11),
    reason="pyproject.toml parsing requires stdlib tomllib (Python 3.11+)",
)


def _call_metadata_method(options: InitOptions | None = None) -> dict[str, object]:
    stub = III.__new__(III)
    stub._options = options or InitOptions()
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


@requires_tomllib
def test_detect_project_name_reads_pyproject_name(tmp_path: Path) -> None:
    (tmp_path / "pyproject.toml").write_text('[project]\nname = "my-pkg"\n')

    assert _detect_project_name(str(tmp_path)) == "my-pkg"


def test_detect_project_name_falls_back_to_cwd_basename_when_no_manifest(
    tmp_path: Path,
) -> None:
    assert _detect_project_name(str(tmp_path)) == tmp_path.name


def test_detect_project_name_falls_back_to_cwd_basename_when_name_missing(
    tmp_path: Path,
) -> None:
    (tmp_path / "pyproject.toml").write_text('[project]\nversion = "1.0.0"\n')

    assert _detect_project_name(str(tmp_path)) == tmp_path.name


def test_detect_project_name_falls_back_to_cwd_basename_when_pyproject_malformed(
    tmp_path: Path,
) -> None:
    (tmp_path / "pyproject.toml").write_text("not valid toml [[[")

    assert _detect_project_name(str(tmp_path)) == tmp_path.name


@requires_tomllib
def test_get_worker_metadata_auto_detects_project_name_from_pyproject(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    (tmp_path / "pyproject.toml").write_text('[project]\nname = "auto-detected-pkg"\n')
    monkeypatch.chdir(tmp_path)

    metadata = _call_metadata_method()

    assert metadata["telemetry"]["project_name"] == "auto-detected-pkg"  # type: ignore[index]


def test_get_worker_metadata_user_provided_project_name_wins(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    (tmp_path / "pyproject.toml").write_text('[project]\nname = "auto-detected-pkg"\n')
    monkeypatch.chdir(tmp_path)
    options = InitOptions(telemetry=TelemetryOptions(project_name="explicit-override"))

    metadata = _call_metadata_method(options)

    assert metadata["telemetry"]["project_name"] == "explicit-override"  # type: ignore[index]


def test_get_worker_metadata_falls_back_to_cwd_basename_without_pyproject(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.chdir(tmp_path)

    metadata = _call_metadata_method()

    assert metadata["telemetry"]["project_name"] == tmp_path.name  # type: ignore[index]
