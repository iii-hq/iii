"""iii-observability: shared OTel + Logger primitives."""

__version__ = "0.13.0.dev1"

from .reconnection import ReconnectionConfig
from .telemetry import (
    current_span_id,
    current_trace_id,
)
from .telemetry_types import OtelConfig
