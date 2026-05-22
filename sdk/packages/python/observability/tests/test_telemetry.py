import pytest
from iii_observability import OtelConfig, ReconnectionConfig, current_span_id, current_trace_id


def test_otel_config_defaults_disabled():
    cfg = OtelConfig()
    assert cfg.enabled is None  # enabled=None means "read from env"


def test_reconnection_config_has_initial_delay():
    cfg = ReconnectionConfig()
    assert cfg.initial_delay_ms > 0


def test_current_ids_none_outside_span():
    assert current_span_id() is None
    assert current_trace_id() is None
