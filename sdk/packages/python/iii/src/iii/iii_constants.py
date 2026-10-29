"""Constants and configuration types for the III SDK (mirrors iii-constants.ts)."""

from dataclasses import dataclass
from typing import Any, Callable, Final, Literal

from iii_helpers.observability import OtelConfig, ReconnectionConfig

IIIConnectionState = Literal["disconnected", "connecting", "connected", "reconnecting", "failed"]

ConnectionStateCallback = Callable[["IIIConnectionState"], None]

DEFAULT_INVOCATION_TIMEOUT_MS = 30000
MAX_QUEUE_SIZE = 1000

#: Engine address used when neither an explicit address nor ``III_URL`` is set.
#: The IPv4 loopback is spelled out on purpose: ``localhost`` can resolve to
#: ``::1`` on a host whose engine only listens on IPv4.
DEFAULT_ENGINE_URL = "ws://127.0.0.1:49134"


DEFAULT_RECONNECTION_CONFIG = ReconnectionConfig()


@dataclass
class TelemetryOptions:
    """Worker metadata reported to the engine (language, framework, project).

    Attributes:
        language: Programming language of the worker.
        project_name: Name of the project this worker belongs to.
        framework: Framework name, if applicable.
    """

    language: str | None = None
    project_name: str | None = None
    framework: str | None = None


@dataclass
class InitOptions:
    """Configuration options passed to ``register_worker``.

    Attributes:
        worker_name: Display name for this worker. A non-empty
            ``III_WORKER_NAME`` overrides it. Defaults to ``hostname:pid``.
        worker_description: One-line, human/LLM-readable summary of what this
            worker does. Surfaces in ``engine::workers::list`` / ``engine::workers::info``.
        namespace: Namespace this worker belongs to. Falls back to the
            ``III_NAMESPACE`` env var; when neither is set the engine applies
            ``default``. It scopes more than the registration: the worker and
            its functions register here, and everything the worker does
            afterwards follows it: ``call`` resolves its target here and
            ``observe`` resolves its source and target here, unless the call
            names another namespace.
        enable_metrics_reporting: Enable worker metrics via OpenTelemetry. Default ``True``.
        invocation_timeout_ms: Default timeout for ``worker.call()`` invocations in milliseconds. Default ``30000``.
        reconnection_config: WebSocket reconnection behavior.
        otel: OpenTelemetry configuration. Enabled by default.
            Set ``{'enabled': False}`` or env ``OTEL_ENABLED=false`` to disable.
        headers: Custom HTTP headers sent during the WebSocket handshake, on the
            main connection and on streaming channels. ``Authorization: Bearer $III_TOKEN``
            is added unless you set ``Authorization`` here.
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
    REGISTER_WORKER: Final[str] = "engine::workers::register"


class EngineSources:
    """Ids of the engine-hosted functions that are called once per event:
    observe them with ``iii.observe(EngineSources.FUNCTIONS_CHANGED, target)``
    (parity with the Node SDK)."""

    FUNCTIONS_CHANGED: Final[str] = "engine::on-functions-changed"
    WORKERS_CHANGED: Final[str] = "engine::on-workers-changed"
    OBSERVERS_CHANGED: Final[str] = "engine::on-observers-changed"
    LOG: Final[str] = "observability::on-log"
    SPAN: Final[str] = "observability::on-span"
    CONFIGURATION_UPDATED: Final[str] = "configuration::on-updated"
    STREAM_CHANGED: Final[str] = "stream::on-changed"
    STREAM_JOIN: Final[str] = "stream::on-join"
    STREAM_LEAVE: Final[str] = "stream::on-leave"
