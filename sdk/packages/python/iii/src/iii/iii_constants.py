"""Constants and configuration types for the III SDK (mirrors iii-constants.ts)."""

from dataclasses import dataclass
from typing import Any, Callable, Final, Literal

from iii_helpers.observability import OtelConfig, ReconnectionConfig

IIIConnectionState = Literal["disconnected", "connecting", "connected", "reconnecting", "failed"]

ConnectionStateCallback = Callable[["IIIConnectionState"], None]

DEFAULT_INVOCATION_TIMEOUT_MS = 30000
MAX_QUEUE_SIZE = 1000


DEFAULT_RECONNECTION_CONFIG = ReconnectionConfig()


@dataclass
class FunctionRef:
    """Reference to a registered function, allowing programmatic unregistration.

    Attributes:
        id: The unique function identifier.
        unregister: Removes this function from the engine.
    """

    id: str
    unregister: Callable[[], None]


@dataclass
class TelemetryOptions:
    """Worker metadata reported to the engine (language, framework, project).

    Attributes:
        language: Programming language of the worker.
        project_name: Name of the project this worker belongs to.
        framework: Framework name, if applicable.
        amplitude_api_key: Amplitude API key for product analytics.
    """

    language: str | None = None
    project_name: str | None = None
    framework: str | None = None
    amplitude_api_key: str | None = None


@dataclass
class InitOptions:
    """Configuration options passed to ``register_worker``.

    Attributes:
        worker_name: Display name for this worker. Defaults to ``hostname:pid``.
        worker_description: One-line, human/LLM-readable summary of what this
            worker does. Surfaces in ``engine::workers::list`` / ``engine::workers::info``.
        namespace: Routing namespace this worker registers its functions into.
            Falls back to the ``III_NAMESPACE`` env var; when neither is set the
            engine applies ``default``.
        enable_metrics_reporting: Enable worker metrics via OpenTelemetry. Default ``True``.
        invocation_timeout_ms: Default timeout for ``worker.trigger()`` invocations in milliseconds. Default ``30000``.
        reconnection_config: WebSocket reconnection behavior.
        otel: OpenTelemetry configuration. Enabled by default.
            Set ``{'enabled': False}`` or env ``OTEL_ENABLED=false`` to disable.
        telemetry: Internal worker metadata reported to the engine.
    """

    worker_name: str | None = None
    worker_description: str | None = None
    namespace: str | None = None
    enable_metrics_reporting: bool = True
    invocation_timeout_ms: int = DEFAULT_INVOCATION_TIMEOUT_MS
    reconnection_config: ReconnectionConfig | None = None
    otel: OtelConfig | dict[str, Any] | None = None
    headers: dict[str, str] | None = None
    telemetry: TelemetryOptions | None = None


class EngineFunctions:
    """Engine function ids for internal operations (parity with the Node SDK)."""

    LIST_FUNCTIONS: Final[str] = "engine::functions::list"
    INFO_FUNCTIONS: Final[str] = "engine::functions::info"
    LIST_WORKERS: Final[str] = "engine::workers::list"
    INFO_WORKERS: Final[str] = "engine::workers::info"
    LIST_TRIGGERS: Final[str] = "engine::triggers::list"
    INFO_TRIGGERS: Final[str] = "engine::triggers::info"
    LIST_REGISTERED_TRIGGERS: Final[str] = "engine::registered-triggers::list"
    INFO_REGISTERED_TRIGGERS: Final[str] = "engine::registered-triggers::info"
    REGISTER_WORKER: Final[str] = "engine::workers::register"


class EngineTriggers:
    """Engine trigger ids (parity with the Node SDK)."""

    FUNCTIONS_AVAILABLE: Final[str] = "engine::functions-available"
    LOG: Final[str] = "log"
