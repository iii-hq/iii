"""Protocol message and register-input types, and the low-level stream refs.

The ref helpers describe streams as they travel on the wire (``{channel_id,
access_key, direction}``). Code that sends and receives streams uses
:class:`iii.InputStream` and :class:`iii.OutputStream` instead: the SDK turns
them into refs and back.
"""

from .iii_types import (
    FEATURE_LARGE_MESSAGES_V1,
    FEATURE_LOCAL_DISPATCH_V1,
    FEATURE_OBSERVE_V1,
    FEATURE_OBSERVE_V2,
    SDK_FEATURES,
    CallRequest,
    ChannelDirection,
    ChannelItem,
    MessageType,
    RegisterFunctionFormat,
    RegisterFunctionInput,
    RegisterFunctionMessage,
    StreamChannelRef,
    TriggerRequest,
)
from .types import StreamChannelRefDict, extract_channel_refs, is_channel_ref

__all__ = [
    "FEATURE_LARGE_MESSAGES_V1",
    "FEATURE_LOCAL_DISPATCH_V1",
    "FEATURE_OBSERVE_V1",
    "FEATURE_OBSERVE_V2",
    "SDK_FEATURES",
    "CallRequest",
    "ChannelDirection",
    "ChannelItem",
    "MessageType",
    "RegisterFunctionFormat",
    "RegisterFunctionInput",
    "RegisterFunctionMessage",
    "StreamChannelRef",
    "StreamChannelRefDict",
    "TriggerRequest",
    "extract_channel_refs",
    "is_channel_ref",
]
