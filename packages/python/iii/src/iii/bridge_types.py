"""Bridge message types."""

from enum import Enum
from typing import Any

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
    EVALUATE_CONDITION = "evaluatecondition"
    CONDITION_RESULT = "conditionresult"


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


class TriggerConfig(BaseModel):
    """Configuration for a single trigger."""

    trigger_type: str
    config: Any
    has_conditions: bool = False


class RegisterTriggerMessage(BaseModel):
    """Message for registering a trigger."""

    model_config = ConfigDict(populate_by_name=True)

    id: str
    function_path: str = Field()
    triggers: list[TriggerConfig]
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


class EvaluateConditionMessage(BaseModel):
    """Message for evaluating a condition."""

    condition_id: str
    trigger_id: str
    trigger_metadata: Any
    input_data: Any
    type: MessageType = MessageType.EVALUATE_CONDITION


class ConditionResultMessage(BaseModel):
    """Message for condition evaluation result."""

    condition_id: str
    passed: bool
    type: MessageType = MessageType.CONDITION_RESULT


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
    | EvaluateConditionMessage
    | ConditionResultMessage
)
