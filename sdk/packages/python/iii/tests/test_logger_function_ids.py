import logging

from iii.logger import Logger


def test_logger_uses_service_name() -> None:
    """Logger stores service_name for use in log records."""
    logger = Logger(service_name="my.function")
    assert logger._service_name == "my.function"


def test_logger_default_service_name() -> None:
    """Logger defaults service_name to empty string."""
    logger = Logger()
    assert logger._service_name == ""


def test_logger_falls_back_to_python_logging_when_otel_not_initialized(
    caplog: logging.LogRecord,
) -> None:
    """When OTel is not initialized Logger falls back to Python logging."""
    logger = Logger(service_name="step.fn")

    with caplog.at_level(logging.DEBUG, logger="iii.logger"):
        logger.info("info message")
        logger.warn("warn message")
        logger.error("error message")
        logger.debug("debug message")

    messages = [r.message for r in caplog.records]
    assert any("info message" in m for m in messages)
    assert any("warn message" in m for m in messages)
    assert any("error message" in m for m in messages)
    assert any("debug message" in m for m in messages)
