"""OTel configuration types for the III Python SDK."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass
class OtelConfig:
    """Configuration for OpenTelemetry initialization."""

    enabled: bool | None = None
    """Enable OTel. Defaults to True. Set OTEL_ENABLED=false/0/no/off to disable."""

    service_name: str | None = None
    """Service name. Defaults to env OTEL_SERVICE_NAME or 'iii-python-sdk'."""

    service_version: str | None = None
    """Service version. Defaults to env SERVICE_VERSION or 'unknown'."""

    service_namespace: str | None = None
    """Service namespace attribute."""

    service_instance_id: str | None = None
    """Service instance ID. Defaults to a random UUID."""

    engine_ws_url: str | None = None
    """III Engine WebSocket URL. Defaults to env III_URL or 'ws://localhost:49134'."""

    fetch_instrumentation_enabled: bool = True
    """Auto-instrument urllib HTTP calls via URLLibInstrumentor. Defaults to True."""

    logs_enabled: bool | None = None
    """Enable OTel log export via EngineLogExporter. Defaults to True when OTel is enabled."""

    logs_flush_interval_ms: int | None = None
    """Log processor flush delay in milliseconds. Defaults to 100ms when not set."""

    logs_batch_size: int | None = None
    """Maximum number of log records exported per batch. Defaults to 1 when not set."""

    metrics_enabled: bool = True
    """Enable OTel metrics export via EngineMetricsExporter. Defaults to True."""

    metrics_export_interval_ms: int = 60000
    """Metrics export interval in milliseconds. Defaults to 60000 (60 seconds)."""
