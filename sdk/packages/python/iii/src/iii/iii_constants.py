"""Constants and configuration types for the III SDK (mirrors iii-constants.ts)."""

from dataclasses import dataclass
from typing import Any, Callable, Literal

from .telemetry_types import OtelConfig

IIIConnectionState = Literal["disconnected", "connecting", "connected", "reconnecting", "failed"]

ConnectionStateCallback = Callable[["IIIConnectionState"], None]

DEFAULT_INVOCATION_TIMEOUT_MS = 30000
MAX_QUEUE_SIZE = 1000


@dataclass
class ReconnectionConfig:
    """Configuration for WebSocket reconnection behavior.

    Attributes:
        initial_delay_ms: Starting delay in milliseconds. Default ``1000``.
        max_delay_ms: Maximum delay cap in milliseconds. Default ``30000``.
        backoff_multiplier: Exponential backoff multiplier. Default ``2.0``.
        jitter_factor: Random jitter factor (0--1). Default ``0.3``.
        max_retries: Maximum retry attempts. ``-1`` for infinite. Default ``-1``.
    """

    initial_delay_ms: int = 1000
    max_delay_ms: int = 30000
    backoff_multiplier: float = 2.0
    jitter_factor: float = 0.3
    max_retries: int = -1


DEFAULT_RECONNECTION_CONFIG = ReconnectionConfig()


@dataclass
class FunctionRef:
    """Reference to a registered function, allowing programmatic unregistration."""

    id: str
    unregister: Callable[[], None]


@dataclass
class TelemetryOptions:
    """Telemetry metadata to be reported to the engine.

    Attributes:
        language: Programming language of the worker (e.g. ``python``).
        project_name: Name of the project this worker belongs to.
        framework: Framework name (e.g. ``motia``) if applicable.
        amplitude_api_key: Amplitude API key for product analytics.
    """

    language: str | None = None
    project_name: str | None = None
    framework: str | None = None
    amplitude_api_key: str | None = None


@dataclass
class InitOptions:
    """Options for configuring the III SDK.

    Attributes:
        worker_name: Display name for this worker. Defaults to ``hostname:pid``.
        enable_metrics_reporting: Enable worker metrics via OpenTelemetry. Default ``True``.
        invocation_timeout_ms: Default timeout for ``trigger()`` in milliseconds. Default ``30000``.
        reconnection_config: WebSocket reconnection behavior.
        otel: OpenTelemetry configuration. Enabled by default.
            Set ``{'enabled': False}`` or env ``OTEL_ENABLED=false`` to disable.
        telemetry: Internal telemetry metadata.
    """

    worker_name: str | None = None
    enable_metrics_reporting: bool = True
    invocation_timeout_ms: int = DEFAULT_INVOCATION_TIMEOUT_MS
    reconnection_config: ReconnectionConfig | None = None
    otel: OtelConfig | dict[str, Any] | None = None
    telemetry: TelemetryOptions | None = None
