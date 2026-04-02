"""Tests for OTel-bridge behavior of Logger."""

from unittest.mock import patch

import pytest


@pytest.fixture(autouse=True)
def reset_otel():
    from iii.telemetry import shutdown_otel

    yield
    shutdown_otel()
    # Reset all OTel global singletons so tests don't bleed state
    try:
        import opentelemetry._logs._internal as _li

        _li._LOGGER_PROVIDER = None
        _li._LOGGER_PROVIDER_SET_ONCE._done = False
    except Exception:
        pass
    try:
        import opentelemetry.trace._internal as _ti

        _ti._TRACER_PROVIDER = None
        _ti._TRACER_PROVIDER_SET_ONCE._done = False
    except Exception:
        pass
    try:
        import opentelemetry.metrics._internal as _mi

        _mi._METER_PROVIDER = None
        _mi._METER_PROVIDER_SET_ONCE._done = False
    except Exception:
        pass


def _setup_in_memory_log_provider():
    from opentelemetry import _logs
    from opentelemetry.sdk._logs import LoggerProvider as SdkLoggerProvider
    from opentelemetry.sdk._logs.export import InMemoryLogRecordExporter, SimpleLogRecordProcessor

    log_exporter = InMemoryLogRecordExporter()
    lp = SdkLoggerProvider()
    lp.add_log_record_processor(SimpleLogRecordProcessor(log_exporter))
    _logs.set_logger_provider(lp)
    return log_exporter


def test_logger_emits_otel_record_when_initialized():
    """Logger.info emits an OTel LogRecord with severity INFO when OTel is active."""
    from iii.logger import Logger
    from iii.telemetry import init_otel
    from iii.telemetry_types import OtelConfig

    log_exporter = _setup_in_memory_log_provider()
    init_otel(OtelConfig(enabled=True, logs_enabled=False))  # skip EngineLogExporter

    logger = Logger(service_name="fn1")
    logger.info("hello world", {"key": "val"})

    records = log_exporter.get_finished_logs()
    assert len(records) == 1
    assert records[0].log_record.body == "hello world"
    assert records[0].log_record.severity_text == "INFO"


def test_logger_emits_warn_severity():
    from opentelemetry._logs import SeverityNumber

    from iii.logger import Logger

    log_exporter = _setup_in_memory_log_provider()

    with patch("iii.logger.is_initialized", return_value=True):
        logger = Logger()
        logger.warn("watch out")

    records = log_exporter.get_finished_logs()
    assert len(records) == 1
    assert records[0].log_record.severity_number == SeverityNumber.WARN


def test_logger_attaches_trace_context_from_active_span():
    """Logger attaches trace_id and span_id from the active OTel span."""
    from opentelemetry import trace
    from opentelemetry.sdk.trace import TracerProvider

    from iii.logger import Logger

    log_exporter = _setup_in_memory_log_provider()

    tracer_provider = TracerProvider()
    trace.set_tracer_provider(tracer_provider)
    tracer = tracer_provider.get_tracer("test")

    with patch("iii.logger.is_initialized", return_value=True):
        with tracer.start_as_current_span("test-span") as span:
            logger = Logger(service_name="fn1")
            logger.info("inside span")

            span_ctx = span.get_span_context()

    records = log_exporter.get_finished_logs()
    assert len(records) == 1
    lr = records[0].log_record
    assert lr.trace_id == span_ctx.trace_id
    assert lr.span_id == span_ctx.span_id


def test_logger_no_trace_context_outside_span():
    """Logger emits zero trace_id/span_id when no active span."""
    from iii.logger import Logger

    log_exporter = _setup_in_memory_log_provider()

    with patch("iii.logger.is_initialized", return_value=True):
        logger = Logger(service_name="fn1")
        logger.info("outside span")

    records = log_exporter.get_finished_logs()
    assert len(records) == 1
    lr = records[0].log_record
    assert lr.trace_id == 0
    assert lr.span_id == 0
