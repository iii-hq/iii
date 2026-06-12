"""III SDK for Python."""

from typing import Any

from .channels import ChannelReader, ChannelWriter
from .errors import InvocationError
from .format_utils import extract_request_format, extract_response_format, python_type_to_format
from .iii import TriggerAction, register_worker
from .iii_constants import (
    EngineFunctions,
    EngineTriggers,
    FunctionRef,
    InitOptions,
    TelemetryOptions,
)
from .iii_types import (
    MessageType,
    MiddlewareFunctionInput,
    RegisterFunctionFormat,
    RegisterFunctionMessage,
    RegisterTriggerInput,
    RegisterTriggerMessage,
    RegisterTriggerTypeInput,
    RegisterTriggerTypeMessage,
    StreamChannelRef,
    TriggerActionEnqueue,
    TriggerActionVoid,
    TriggerRequest,
)
from .stream import (
    IStream,
    StreamChangeEvent,
    StreamChangeEventDetail,
    StreamContext,
    StreamJoinLeaveEvent,
    StreamJoinLeaveTriggerConfig,
    StreamTriggerConfig,
)
from .triggers import Trigger, TriggerConfig, TriggerHandler, TriggerTypeRef
from .types import (
    Channel,
    IIIClient,
    InternalHttpRequest,
    RemoteFunctionHandler,
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
    # Engine
    "EngineFunctions",
    "EngineTriggers",
    # RBAC types
    "MiddlewareFunctionInput",
    # Message types
    "MessageType",
    "RegisterFunctionFormat",
    "RegisterFunctionMessage",
    "RegisterTriggerInput",
    "RegisterTriggerMessage",
    "RegisterTriggerTypeInput",
    "RegisterTriggerTypeMessage",
    "StreamChannelRef",
    "TriggerActionEnqueue",
    "TriggerActionVoid",
    "TriggerRequest",
    # Triggers
    "Trigger",
    "TriggerConfig",
    "TriggerHandler",
    "TriggerTypeRef",
    # Types
    "Channel",
    "IIIClient",
    "InternalHttpRequest",
    "RemoteFunctionHandler",
    "StreamRequest",
    "StreamResponse",
    # Stream
    "IStream",
    "StreamChangeEvent",
    "StreamChangeEventDetail",
    "StreamContext",
    "StreamJoinLeaveEvent",
    "StreamJoinLeaveTriggerConfig",
    "StreamTriggerConfig",
    # Format extraction
    "extract_request_format",
    "extract_response_format",
    "python_type_to_format",
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
