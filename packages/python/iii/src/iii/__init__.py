"""III SDK for Python."""

import logging

from .bridge import Bridge, BridgeOptions
from .bridge_types import FunctionInfo, WorkerInfo, WorkerStatus
from .context import Context, get_context, with_context
from .logger import Logger
from .streams import (
    IStream,
    StreamAuthInput,
    StreamAuthResult,
    StreamDeleteInput,
    StreamGetGroupInput,
    StreamGetInput,
    StreamJoinLeaveEvent,
    StreamJoinResult,
    StreamListGroupsInput,
    StreamSetInput,
    StreamSetResult,
    StreamUpdateInput,
    UpdateDecrement,
    UpdateIncrement,
    UpdateMerge,
    UpdateOp,
    UpdateRemove,
    UpdateSet,
)
from .types import ApiRequest, ApiResponse, FunctionsAvailableCallback, RemoteFunctionHandler


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
    # Core
    "Bridge",
    "BridgeOptions",
    "Logger",
    "Context",
    "get_context",
    "with_context",
    # API types
    "ApiRequest",
    "ApiResponse",
    # Bridge types
    "FunctionInfo",
    "WorkerInfo",
    "WorkerStatus",
    # Stream types
    "IStream",
    "StreamAuthInput",
    "StreamAuthResult",
    "StreamDeleteInput",
    "StreamGetGroupInput",
    "StreamGetInput",
    "StreamJoinLeaveEvent",
    "StreamJoinResult",
    "StreamListGroupsInput",
    "StreamSetInput",
    "StreamSetResult",
    "StreamUpdateInput",
    "UpdateDecrement",
    "UpdateIncrement",
    "UpdateMerge",
    "UpdateOp",
    "UpdateRemove",
    "UpdateSet",
    # Callbacks
    "FunctionsAvailableCallback",
    "RemoteFunctionHandler",
    # Utility
    "configure_logging",
]
