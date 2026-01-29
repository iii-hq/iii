"""Bridge message types."""

from enum import Enum
from typing import Any, Literal

from pydantic import BaseModel, ConfigDict, Field


class MessageType(str, Enum):
    """Message types for bridge communication."""

    REGISTER_FUNCTION = "registerfunction"
    REGISTER_SERVICE = "registerservice"
    INVOKE_FUNCTION = "invokefunction"
    INVOCATION_RESULT = "invocationresult"
    REGISTER_TRIGGER_TYPE = "registertriggertype"
    REGISTER_TRIGGER = "registertrigger"
    UNREGISTER_TRIGGER = "unregistertrigger"
    UNREGISTER_TRIGGER_TYPE = "unregistertriggertype"
    TRIGGER_REGISTRATION_RESULT = "triggerregistrationresult"


class RegisterTriggerTypeMessage(BaseModel):
    """Message for registering a trigger type."""

    id: str
    description: str
    type: MessageType = MessageType.REGISTER_TRIGGER_TYPE


class UnregisterTriggerTypeMessage(BaseModel):
    """Message for unregistering a trigger type."""

    id: str
    type: MessageType = MessageType.UNREGISTER_TRIGGER_TYPE


class UnregisterTriggerMessage(BaseModel):
    """Message for unregistering a trigger."""

    id: str
    type: MessageType = MessageType.UNREGISTER_TRIGGER


class TriggerRegistrationResultMessage(BaseModel):
    """Message for trigger registration result."""

    model_config = ConfigDict(populate_by_name=True)

    id: str
    trigger_type: str = Field()
    function_path: str = Field()
    result: Any = None
    error: Any = None
    type: MessageType = MessageType.TRIGGER_REGISTRATION_RESULT


class RegisterTriggerMessage(BaseModel):
    """Message for registering a trigger."""

    model_config = ConfigDict(populate_by_name=True)

    id: str
    trigger_type: str = Field()
    function_path: str = Field()
    config: Any
    type: MessageType = MessageType.REGISTER_TRIGGER


class RegisterServiceMessage(BaseModel):
    """Message for registering a service."""

    model_config = ConfigDict(populate_by_name=True)

    id: str
    description: str | None = None
    parent_service_id: str | None = Field(default=None)
    type: MessageType = MessageType.REGISTER_SERVICE


class RegisterFunctionFormat(BaseModel):
    """Format definition for function parameters."""

    name: str
    type: str  # 'string' | 'number' | 'boolean' | 'object' | 'array' | 'null' | 'map'
    description: str | None = None
    body: list["RegisterFunctionFormat"] | None = None
    items: "RegisterFunctionFormat | None" = None
    required: bool = False


class RegisterFunctionMessage(BaseModel):
    """Message for registering a function."""

    model_config = ConfigDict(populate_by_name=True)

    function_path: str = Field()
    description: str | None = None
    request_format: RegisterFunctionFormat | None = Field(default=None)
    response_format: RegisterFunctionFormat | None = Field(default=None)
    metadata: dict[str, Any] | None = None
    type: MessageType = MessageType.REGISTER_FUNCTION


class InvokeFunctionMessage(BaseModel):
    """Message for invoking a function."""

    model_config = ConfigDict(populate_by_name=True)

    function_path: str = Field()
    data: Any
    invocation_id: str | None = Field(default=None)
    type: MessageType = MessageType.INVOKE_FUNCTION


class InvocationResultMessage(BaseModel):
    """Message for invocation result."""

    model_config = ConfigDict(populate_by_name=True)

    invocation_id: str = Field()
    function_path: str = Field()
    result: Any = None
    error: Any = None
    type: MessageType = MessageType.INVOCATION_RESULT


class FunctionInfo(BaseModel):
    """Information about a registered function."""

    function_path: str
    description: str | None = None
    request_format: RegisterFunctionFormat | None = None
    response_format: RegisterFunctionFormat | None = None
    metadata: dict[str, Any] | None = None


WorkerStatus = Literal["connected", "available", "busy", "disconnected"]


class WorkerInfo(BaseModel):
    """Information about a connected worker."""

    id: str
    name: str | None = None
    runtime: str | None = None
    version: str | None = None
    os: str | None = None
    ip_address: str | None = None
    status: WorkerStatus
    connected_at_ms: int
    function_count: int
    functions: list[str]
    active_invocations: int


BridgeMessage = (
    RegisterFunctionMessage
    | InvokeFunctionMessage
    | InvocationResultMessage
    | RegisterServiceMessage
    | RegisterTriggerMessage
    | RegisterTriggerTypeMessage
    | UnregisterTriggerMessage
    | UnregisterTriggerTypeMessage
    | TriggerRegistrationResultMessage
)
