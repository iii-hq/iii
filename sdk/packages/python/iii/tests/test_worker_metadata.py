from __future__ import annotations

import asyncio
import json
import os
import sys
import threading
from pathlib import Path

import pytest

from iii import InitOptions, TriggerAction
from iii.errors import RegistrationRejectedError
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


def test_get_worker_metadata_name_defaults_from_iii_worker_name_env(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("III_WORKER_NAME", "managed-worker")

    metadata = _call_metadata_method()

    assert metadata["name"] == "managed-worker"


def test_get_worker_metadata_explicit_worker_name_wins_over_env(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("III_WORKER_NAME", "managed-worker")

    metadata = _call_metadata_method(InitOptions(worker_name="explicit-name"))

    assert metadata["name"] == "explicit-name"


def test_get_worker_metadata_name_falls_back_to_hostname_pid(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.delenv("III_WORKER_NAME", raising=False)

    metadata = _call_metadata_method()

    assert metadata["name"].endswith(f":{os.getpid()}")


def test_get_worker_metadata_namespace_is_none_when_env_unset(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.delenv("III_NAMESPACE", raising=False)

    metadata = _call_metadata_method()

    assert "namespace" in metadata
    assert metadata["namespace"] is None


def test_get_worker_metadata_forwards_iii_namespace_env_var(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("III_NAMESPACE", "orders")

    metadata = _call_metadata_method()

    assert metadata["namespace"] == "orders"


def test_get_worker_metadata_explicit_namespace_wins_over_env(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("III_NAMESPACE", "orders")

    metadata = _call_metadata_method(InitOptions(namespace="analytics"))

    assert metadata["namespace"] == "analytics"


def _capture_triggered_message(request: dict[str, object]) -> dict[str, object]:
    """Drive the real ``trigger_async`` void path and return the serialized wire dict."""
    stub = III.__new__(III)
    stub._options = InitOptions()
    captured: dict[str, object] = {}

    async def fake_send(msg: object) -> None:
        captured["wire"] = stub._to_dict(msg)

    stub._send = fake_send  # type: ignore[method-assign]
    asyncio.run(stub.trigger_async(request))
    return captured["wire"]  # type: ignore[return-value]


def test_trigger_serializes_namespace_when_set() -> None:
    wire = _capture_triggered_message(
        {
            "function_id": "state::get",
            "payload": {},
            "action": TriggerAction.Void(),
            "namespace": "analytics",
        }
    )

    assert wire["namespace"] == "analytics"


def test_trigger_omits_namespace_when_absent() -> None:
    wire = _capture_triggered_message(
        {
            "function_id": "state::get",
            "payload": {},
            "action": TriggerAction.Void(),
        }
    )

    assert "namespace" not in wire


def test_registration_rejected_is_fatal_and_does_not_reconnect() -> None:
    stub = III.__new__(III)
    stub._running = True
    stub._reconnect_task = None
    stub._ws = None
    stub._fatal_error = None
    stub._pending = {}
    stub._connection_state = "connected"
    stub._connected_event = threading.Event()
    stub._connection_listeners = []
    observed: list[str] = []
    stub._connection_listeners.append(lambda state: observed.append(state))

    asyncio.run(
        stub._handle_message(
            json.dumps(
                {
                    "type": "registrationrejected",
                    "code": "WORKER_NAMESPACE_CONFLICT",
                    "namespace": "orders",
                    "worker_name": "state",
                    "owner_worker_id": "owner-123",
                }
            )
        )
    )

    # Fatal: the worker stops and never schedules a reconnect.
    assert stub._running is False
    assert stub._reconnect_task is None
    assert "failed" in observed

    err = stub._fatal_error
    assert isinstance(err, RegistrationRejectedError)
    assert err.code == "WORKER_NAMESPACE_CONFLICT"
    assert err.namespace == "orders"
    assert err.worker_name == "state"
    assert err.owner_worker_id == "owner-123"


def test_get_worker_metadata_omits_description_when_unset() -> None:
    metadata = _call_metadata_method()

    assert "description" not in metadata


def test_get_worker_metadata_includes_worker_description_when_set() -> None:
    metadata = _call_metadata_method(InitOptions(worker_description="resizes images"))

    assert metadata["description"] == "resizes images"


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
