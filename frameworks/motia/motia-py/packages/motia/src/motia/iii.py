"""III SDK client initialization for Motia framework."""

import os
from pathlib import Path
from typing import Any

try:
    import tomllib  # type: ignore[import-not-found]
except ImportError:
    import tomli as tomllib  # type: ignore[import-not-found]

from iii import IIIClient, register_worker
from iii.iii_constants import InitOptions, TelemetryOptions

_engine_ws_url = os.environ.get("III_URL", "ws://localhost:49134")
_instance: IIIClient | None = None


def _read_project_name() -> str | None:
    """Walk up from cwd (max 1 parent) to find the nearest pyproject.toml and extract the project name."""
    max_depth = 1
    directory = Path.cwd()
    for _ in range(max_depth + 1):
        pyproject = directory / "pyproject.toml"
        if pyproject.exists():
            try:
                with open(pyproject, "rb") as f:
                    data = tomllib.load(f)
                name = data.get("project", {}).get("name") or data.get("tool", {}).get("poetry", {}).get("name")
                if name:
                    return str(name)
            except Exception:
                pass
        parent = directory.parent
        if parent == directory:
            break
        directory = parent
    return None


def _create_iii(otel_config: dict[str, Any] | None = None) -> IIIClient:
    telemetry = TelemetryOptions(
        framework="motia",
        project_name=_read_project_name(),
    )
    return register_worker(_engine_ws_url, InitOptions(telemetry=telemetry, otel=otel_config))


def get_instance() -> IIIClient:
    """Get the III SDK singleton instance.

    Creates a default instance if none exists.
    """
    global _instance
    if _instance is None:
        _instance = _create_iii()
    return _instance


def init_iii(otel_config: dict[str, Any] | None = None) -> IIIClient:
    """Initialize the III SDK with optional OpenTelemetry configuration.

    Args:
        otel_config: Optional OpenTelemetry configuration dict.

    Returns:
        The initialized III SDK instance.
    """
    global _instance
    _instance = _create_iii(otel_config)
    return _instance
