"""Tests for OtelConfig dataclass."""

from iii.telemetry_types import OtelConfig


def test_otel_config_defaults():
    config = OtelConfig()
    assert config.enabled is None
    assert config.service_name is None
    assert config.engine_ws_url is None
    assert config.fetch_instrumentation_enabled is True


def test_otel_config_explicit_values():
    config = OtelConfig(
        enabled=True,
        service_name="my-service",
        engine_ws_url="ws://localhost:49134",
        fetch_instrumentation_enabled=False,
    )
    assert config.enabled is True
    assert config.service_name == "my-service"
    assert config.fetch_instrumentation_enabled is False


def test_otlp_endpoint_removed():
    """otlp_endpoint is gone; engine_ws_url is the replacement."""
    cfg = OtelConfig()
    assert not hasattr(cfg, "otlp_endpoint")


def test_logs_enabled_defaults_to_none():
    cfg = OtelConfig()
    assert cfg.logs_enabled is None


def test_engine_ws_url_exists():
    cfg = OtelConfig(engine_ws_url="ws://custom:1234")
    assert cfg.engine_ws_url == "ws://custom:1234"


def test_logs_batch_config_defaults_to_none():
    cfg = OtelConfig()
    assert cfg.logs_flush_interval_ms is None
    assert cfg.logs_batch_size is None


def test_logs_batch_config_explicit():
    cfg = OtelConfig(logs_flush_interval_ms=500, logs_batch_size=10)
    assert cfg.logs_flush_interval_ms == 500
    assert cfg.logs_batch_size == 10
