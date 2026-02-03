import logging

from .bridge import Bridge, BridgeOptions
from .bridge_types import FunctionInfo, WorkerInfo, WorkerStatus
from .context import Context, get_context, with_context
from .logger import Logger
from .metrics import (
    MetricsReporter,
    WorkerMetricsCollector,
    collect_metrics,
    get_metrics_collector,
    is_kubernetes,
    record_invocation,
)
from .telemetry import (
    EngineMetricsExporter,
    WorkerGaugesOptions,
    register_system_gauges,
    register_worker_gauges,
    stop_worker_gauges,
)
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
    if format is None:
        format = "%(asctime)s [%(levelname)s] %(name)s: %(message)s"

    logging.basicConfig(level=level, format=format)
    logging.getLogger("iii").setLevel(level)


__all__ = [
    "Bridge",
    "BridgeOptions",
    "Logger",
    "Context",
    "get_context",
    "with_context",
    "ApiRequest",
    "ApiResponse",
    "FunctionInfo",
    "WorkerInfo",
    "WorkerStatus",
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
    "FunctionsAvailableCallback",
    "RemoteFunctionHandler",
    "configure_logging",
    "collect_metrics",
    "record_invocation",
    "is_kubernetes",
    "MetricsReporter",
    "WorkerMetricsCollector",
    "get_metrics_collector",
    "register_system_gauges",
    "register_worker_gauges",
    "stop_worker_gauges",
    "WorkerGaugesOptions",
    "EngineMetricsExporter",
]
