"""Logger implementation for the III SDK."""

import logging
from typing import Any, Callable

from pydantic import BaseModel, ConfigDict, Field

log = logging.getLogger("iii.logger")


class LoggerParams(BaseModel):
    """Parameters for logger invocation."""

    model_config = ConfigDict(populate_by_name=True)

    message: str
    trace_id: str = Field(default="", serialization_alias="trace_id")
    function_name: str = Field(default="", serialization_alias="function_name")
    data: Any = None


LoggerInvoker = Callable[[str, dict[str, Any]], None]


class Logger:
    """Logger that sends log messages through the bridge."""

    def __init__(
        self,
        invoker: LoggerInvoker | None = None,
        trace_id: str | None = None,
        function_name: str | None = None,
    ) -> None:
        self._invoker = invoker
        self._trace_id = trace_id or ""
        self._function_name = function_name or ""

    def _build_params(self, message: str, data: Any = None) -> dict[str, Any]:
        """Build logger params dict."""
        return {
            "message": message,
            "trace_id": self._trace_id,
            "function_name": self._function_name,
            "data": data,
        }

    def info(self, message: str, data: Any = None) -> None:
        """Log an info message."""
        if self._invoker:
            self._invoker("logger.info", self._build_params(message, data))
        log.info(f"[{self._function_name}] {message}", extra={"data": data})

    def warn(self, message: str, data: Any = None) -> None:
        """Log a warning message."""
        if self._invoker:
            self._invoker("logger.warn", self._build_params(message, data))
        log.warning(f"[{self._function_name}] {message}", extra={"data": data})

    def error(self, message: str, data: Any = None) -> None:
        """Log an error message."""
        if self._invoker:
            self._invoker("logger.error", self._build_params(message, data))
        log.error(f"[{self._function_name}] {message}", extra={"data": data})
