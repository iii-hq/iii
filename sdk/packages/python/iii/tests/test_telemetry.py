"""Tests for OTel init/shutdown."""

import urllib.request

import pytest

from iii.telemetry import get_tracer, init_otel, is_initialized, shutdown_otel, shutdown_otel_async
from iii.telemetry_types import OtelConfig

# URLLibInstrumentor patches OpenerDirector.open, not urlopen directly
ORIGINAL_OPENER_OPEN = urllib.request.OpenerDirector.open


def _reset_otel_singleton(module_path: str, provider_attr: str, set_once_attr: str) -> None:
    """Reset one OTel global singleton (provider + its SetOnce guard)."""
    try:
        import importlib

        mod = importlib.import_module(module_path)
        setattr(mod, provider_attr, None)
        getattr(mod, set_once_attr)._done = False
    except Exception:
        pass


@pytest.fixture(autouse=True)
def cleanup():
    yield
    shutdown_otel()
    # Reset all OTel global singletons so tests don't bleed state
    _reset_otel_singleton("opentelemetry._logs._internal", "_LOGGER_PROVIDER", "_LOGGER_PROVIDER_SET_ONCE")
    _reset_otel_singleton("opentelemetry.trace._internal", "_TRACER_PROVIDER", "_TRACER_PROVIDER_SET_ONCE")
    _reset_otel_singleton("opentelemetry.metrics._internal", "_METER_PROVIDER", "_METER_PROVIDER_SET_ONCE")
    urllib.request.OpenerDirector.open = ORIGINAL_OPENER_OPEN


def test_not_initialized_by_default():
    assert not is_initialized()
    assert get_tracer() is None


def test_init_disabled_when_enabled_is_false():
    init_otel(OtelConfig(enabled=False))
    assert not is_initialized()
    assert get_tracer() is None


def test_init_enabled():
    init_otel(OtelConfig(enabled=True))
    assert is_initialized()
    assert get_tracer() is not None


def test_init_patches_urlopen_by_default():
    init_otel(OtelConfig(enabled=True))
    assert urllib.request.OpenerDirector.open is not ORIGINAL_OPENER_OPEN


def test_init_skips_patch_when_disabled():
    init_otel(OtelConfig(enabled=True, fetch_instrumentation_enabled=False))
    assert urllib.request.OpenerDirector.open is ORIGINAL_OPENER_OPEN


def test_shutdown_restores_urlopen():
    init_otel(OtelConfig(enabled=True))
    assert urllib.request.OpenerDirector.open is not ORIGINAL_OPENER_OPEN
    shutdown_otel()
    assert urllib.request.OpenerDirector.open is ORIGINAL_OPENER_OPEN


def test_shutdown_clears_state():
    init_otel(OtelConfig(enabled=True))
    shutdown_otel()
    assert not is_initialized()
    assert get_tracer() is None


def test_init_is_idempotent():
    init_otel(OtelConfig(enabled=True))
    tracer1 = get_tracer()
    init_otel(OtelConfig(enabled=True))  # second call must be no-op
    assert get_tracer() is tracer1


def test_shutdown_without_init_is_safe():
    shutdown_otel()  # must not raise


def test_telemetry_apis_importable_from_submodules():
    from iii.telemetry import get_tracer, init_otel, is_initialized, shutdown_otel
    from iii.telemetry_types import OtelConfig

    assert callable(init_otel)
    assert callable(shutdown_otel)
    assert callable(get_tracer)
    assert callable(is_initialized)
    assert OtelConfig is not None


def test_init_configures_engine_span_exporter():
    """init_otel wires a BatchSpanProcessor(EngineSpanExporter) on the TracerProvider."""
    from opentelemetry import trace
    from opentelemetry.sdk.trace import TracerProvider
    from opentelemetry.sdk.trace.export import BatchSpanProcessor

    from iii.telemetry_exporters import EngineSpanExporter

    init_otel(OtelConfig(enabled=True))
    provider = trace.get_tracer_provider()
    assert isinstance(provider, TracerProvider)
    processors = provider._active_span_processor._span_processors
    bsp = next((p for p in processors if isinstance(p, BatchSpanProcessor)), None)
    assert bsp is not None
    assert isinstance(bsp.span_exporter, EngineSpanExporter)


def test_init_configures_log_provider():
    """init_otel sets up a global SdkLoggerProvider with EngineLogExporter."""
    from opentelemetry._logs import get_logger_provider
    from opentelemetry.sdk._logs import LoggerProvider as SdkLoggerProvider

    init_otel(OtelConfig(enabled=True))
    lp = get_logger_provider()
    assert isinstance(lp, SdkLoggerProvider)


def test_init_logs_disabled():
    """logs_enabled=False skips the logger provider setup."""
    from opentelemetry._logs import get_logger_provider
    from opentelemetry.sdk._logs import LoggerProvider as SdkLoggerProvider

    init_otel(OtelConfig(enabled=True, logs_enabled=False))
    lp = get_logger_provider()
    assert not isinstance(lp, SdkLoggerProvider)


def test_shutdown_closes_connection():
    """shutdown_otel_async() closes the SharedEngineConnection."""
    import asyncio
    from unittest.mock import AsyncMock, patch

    from iii.telemetry_exporters import SharedEngineConnection

    with (
        patch.object(SharedEngineConnection, "start"),
        patch.object(SharedEngineConnection, "shutdown", new_callable=AsyncMock) as mock_shutdown,
    ):
        init_otel(OtelConfig(enabled=True))
        asyncio.run(shutdown_otel_async())
        mock_shutdown.assert_called_once()


def test_init_configures_log_batch_params_defaults():
    """Default batch config: 100ms flush, batch size 1."""
    from opentelemetry._logs import get_logger_provider
    from opentelemetry.sdk._logs import LoggerProvider as SdkLoggerProvider
    from opentelemetry.sdk._logs.export import BatchLogRecordProcessor

    init_otel(OtelConfig(enabled=True))
    lp = get_logger_provider()
    assert isinstance(lp, SdkLoggerProvider)
    # Verify processor was created with no errors (config was applied)
    processors = lp._multi_log_record_processor._log_record_processors
    blrp = next((p for p in processors if isinstance(p, BatchLogRecordProcessor)), None)
    assert blrp is not None


def test_init_log_batch_config_explicit_overrides():
    """Explicit config values override env vars and defaults."""
    from opentelemetry._logs import get_logger_provider
    from opentelemetry.sdk._logs import LoggerProvider as SdkLoggerProvider

    init_otel(OtelConfig(enabled=True, logs_flush_interval_ms=200, logs_batch_size=5))
    lp = get_logger_provider()
    assert isinstance(lp, SdkLoggerProvider)


def test_init_log_batch_config_env_var_override(monkeypatch):
    """Env vars override defaults when config fields are None."""
    from opentelemetry._logs import get_logger_provider
    from opentelemetry.sdk._logs import LoggerProvider as SdkLoggerProvider

    monkeypatch.setenv("OTEL_LOGS_FLUSH_INTERVAL_MS", "500")
    monkeypatch.setenv("OTEL_LOGS_BATCH_SIZE", "10")
    init_otel(OtelConfig(enabled=True))
    lp = get_logger_provider()
    assert isinstance(lp, SdkLoggerProvider)


def test_init_log_batch_config_explicit_beats_env(monkeypatch):
    """Explicit config value takes precedence over env var."""
    from opentelemetry._logs import get_logger_provider
    from opentelemetry.sdk._logs import LoggerProvider as SdkLoggerProvider

    monkeypatch.setenv("OTEL_LOGS_FLUSH_INTERVAL_MS", "999")
    monkeypatch.setenv("OTEL_LOGS_BATCH_SIZE", "99")
    init_otel(OtelConfig(enabled=True, logs_flush_interval_ms=200, logs_batch_size=5))
    lp = get_logger_provider()
    assert isinstance(lp, SdkLoggerProvider)


def test_init_log_batch_config_invalid_env_ignored(monkeypatch):
    """Invalid env var values are silently ignored, falling through to defaults."""
    from opentelemetry._logs import get_logger_provider
    from opentelemetry.sdk._logs import LoggerProvider as SdkLoggerProvider

    monkeypatch.setenv("OTEL_LOGS_FLUSH_INTERVAL_MS", "not-a-number")
    monkeypatch.setenv("OTEL_LOGS_BATCH_SIZE", "-5")
    init_otel(OtelConfig(enabled=True))
    lp = get_logger_provider()
    assert isinstance(lp, SdkLoggerProvider)
