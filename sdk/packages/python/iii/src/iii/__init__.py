"""III SDK for Python."""

from .baggage_span_processor import DEFAULT_ALLOWLIST, BaggageSpanProcessor
from .channels import ChannelReader, ChannelWriter
from .errors import IIIForbiddenError, IIIInvocationError, IIITimeoutError
from .format_utils import extract_request_format, extract_response_format, python_type_to_format
from .iii import TriggerAction, register_worker
from .iii_constants import FunctionRef, InitOptions, ReconnectionConfig, TelemetryOptions
from .iii_types import (
    AuthInput,
    AuthResult,
    EnqueueResult,
    FunctionInfo,
    HttpAuthConfig,
    HttpInvocationConfig,
    MessageType,
    MiddlewareFunctionInput,
    OnFunctionRegistrationInput,
    OnFunctionRegistrationResult,
    OnTriggerRegistrationInput,
    OnTriggerRegistrationResult,
    OnTriggerTypeRegistrationInput,
    OnTriggerTypeRegistrationResult,
    RegisterFunctionFormat,
    RegisterFunctionMessage,
    RegisterTriggerInput,
    RegisterTriggerMessage,
    RegisterTriggerTypeInput,
    RegisterTriggerTypeMessage,
    StreamChannelRef,
    TriggerActionEnqueue,
    TriggerActionVoid,
    TriggerInfo,
    TriggerRequest,
    TriggerTypeInfo,
)
from .logger import Logger
from .payload import (
    REDACTED_PLACEHOLDER,
    redact,
    redact_and_truncate,
    resolve_max_bytes_from_env,
)
from .span_ops import (
    current_span_is_recording,
    record_span_event,
    set_current_span_attribute,
    set_current_span_error,
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
from .telemetry_types import OtelConfig
from .triggers import Trigger, TriggerConfig, TriggerHandler, TriggerTypeRef
from .types import (
    ApiRequest,
    ApiResponse,
    Channel,
    HttpRequest,
    HttpResponse,
    IIIClient,
    InternalHttpRequest,
    RemoteFunctionHandler,
)
from .utils import http

__all__ = [
    # Telemetry helpers
    "BaggageSpanProcessor",
    "DEFAULT_ALLOWLIST",
    "REDACTED_PLACEHOLDER",
    "current_span_is_recording",
    "record_span_event",
    "set_current_span_attribute",
    "set_current_span_error",
    "redact",
    "redact_and_truncate",
    "resolve_max_bytes_from_env",
    # Channels
    "ChannelReader",
    "ChannelWriter",
    # Errors
    "IIIForbiddenError",
    "IIIInvocationError",
    "IIITimeoutError",
    # Core
    "FunctionRef",
    "InitOptions",
    "OtelConfig",
    "ReconnectionConfig",
    "register_worker",
    "TelemetryOptions",
    "TriggerAction",
    # RBAC types
    "AuthInput",
    "AuthResult",
    "MiddlewareFunctionInput",
    "OnFunctionRegistrationInput",
    "OnFunctionRegistrationResult",
    "OnTriggerRegistrationInput",
    "OnTriggerRegistrationResult",
    "OnTriggerTypeRegistrationInput",
    "OnTriggerTypeRegistrationResult",
    # Message types
    "EnqueueResult",
    "FunctionInfo",
    "HttpAuthConfig",
    "HttpInvocationConfig",
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
    "TriggerInfo",
    "TriggerRequest",
    "TriggerTypeInfo",
    # Logger
    "Logger",
    # Triggers
    "Trigger",
    "TriggerConfig",
    "TriggerHandler",
    "TriggerTypeRef",
    # Types
    "ApiRequest",
    "ApiResponse",
    "Channel",
    "HttpRequest",
    "HttpResponse",
    "IIIClient",
    "InternalHttpRequest",
    "RemoteFunctionHandler",
    # Stream
    "IStream",
    "StreamChangeEvent",
    "StreamChangeEventDetail",
    "StreamContext",
    "StreamJoinLeaveEvent",
    "StreamJoinLeaveTriggerConfig",
    "StreamTriggerConfig",
    # Utilities
    "http",
    # Format extraction
    "extract_request_format",
    "extract_response_format",
    "python_type_to_format",
]
