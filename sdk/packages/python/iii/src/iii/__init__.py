"""III SDK for Python."""

from typing import Any

from .channels import ChannelReader, ChannelWriter
from .errors import InvocationError
from .iii import TriggerAction, register_worker
from .iii_constants import (
    FunctionRef,
    InitOptions,
    TelemetryOptions,
)
from .iii_types import (
    MiddlewareFunctionInput,
    StreamChannelRef,
    TriggerActionEnqueue,
)
from .stream import IStream
from .triggers import Trigger, TriggerConfig, TriggerHandler, TriggerTypeRef
from .types import (
    Channel,
    IIIClient,
    StreamRequest,
    StreamResponse,
)

__all__ = [
    # Channels
    "ChannelReader",
    "ChannelWriter",
    # Errors
    "InvocationError",
    # Core
    "FunctionRef",
    "InitOptions",
    "register_worker",
    "TelemetryOptions",
    "TriggerAction",
    # RBAC types
    "MiddlewareFunctionInput",
    # Message types
    "StreamChannelRef",
    "TriggerActionEnqueue",
    # Triggers
    "Trigger",
    "TriggerConfig",
    "TriggerHandler",
    "TriggerTypeRef",
    # Types
    "Channel",
    "IIIClient",
    "StreamRequest",
    "StreamResponse",
    # Stream
    "IStream",
]


def __getattr__(name: str) -> Any:
    if name == "IIIInvocationError":
        import warnings

        warnings.warn(
            "IIIInvocationError is deprecated; import InvocationError from iii.errors",
            DeprecationWarning,
            stacklevel=2,
        )
        return InvocationError
    raise AttributeError(f"module {__name__!r} has no attribute {name!r}")
