"""III SDK for Python."""

import logging

from .bridge import Bridge
from .context import Context, get_context, with_context
from .logger import Logger
from .types import ApiRequest, ApiResponse


def configure_logging(level: int = logging.INFO, format: str | None = None) -> None:
    """Configure logging for the III SDK.
    
    Args:
        level: Logging level (e.g., logging.DEBUG, logging.INFO)
        format: Log format string. Defaults to a simple format.
    """
    if format is None:
        format = "%(asctime)s [%(levelname)s] %(name)s: %(message)s"
    
    logging.basicConfig(level=level, format=format)
    logging.getLogger("iii").setLevel(level)


__all__ = [
    "Bridge",
    "Logger",
    "Context",
    "get_context",
    "with_context",
    "ApiRequest",
    "ApiResponse",
    "configure_logging",
]
