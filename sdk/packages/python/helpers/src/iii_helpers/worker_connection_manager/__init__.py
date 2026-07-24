"""iii worker connection manager helpers."""

from typing import Any

from pydantic import BaseModel, Field

__all__ = [
    "AuthInput",
    "AuthResult",
    "OnFunctionRegistrationInput",
    "OnFunctionRegistrationResult",
    "OnTriggerRegistrationInput",
    "OnTriggerRegistrationResult",
    "OnTriggerTypeRegistrationInput",
    "OnTriggerTypeRegistrationResult",
]


class AuthInput(BaseModel):
    """Input passed to the RBAC auth function during WebSocket upgrade.

    Contains the HTTP headers, query parameters, and client IP from the
    connecting worker's upgrade request.

    Attributes:
        headers: HTTP headers from the WebSocket upgrade request.
        query_params: Query parameters from the upgrade URL. Each key maps to
            a list of values to support repeated keys.
        ip_address: IP address of the connecting client.
    """

    headers: dict[str, str] = Field(description="HTTP headers from the WebSocket upgrade request.")
    query_params: dict[str, list[str]] = Field(
        description="Query parameters from the upgrade URL. Each key maps to a list of values.",
    )
    ip_address: str = Field(description="IP address of the connecting client.")


class AuthResult(BaseModel):
    """Return value from the RBAC auth function.

    Controls which functions the authenticated worker can invoke and what
    context is forwarded to the middleware.

    Attributes:
        allowed_functions: Additional function IDs to allow beyond ``expose_functions``.
        forbidden_functions: Function IDs to deny even if they match ``expose_functions``.
        allowed_trigger_types: Trigger type IDs the worker may register triggers for.
            When ``None``, all types are allowed.
        allow_trigger_type_registration: Whether the worker may register new trigger types.
        allow_function_registration: Whether the worker may register new functions. Defaults to ``True`` if omitted.
        function_registration_prefix: Optional prefix applied to all function IDs registered
            by this worker.
        context: Arbitrary context forwarded to the middleware function on every invocation.
    """

    allowed_functions: list[str] = Field(
        default_factory=list,
        description="Additional function IDs to allow beyond ``expose_functions``.",
    )
    forbidden_functions: list[str] = Field(
        default_factory=list,
        description="Function IDs to deny even if they match ``expose_functions``.",
    )
    allowed_trigger_types: list[str] | None = Field(
        default=None,
        description="Trigger type IDs the worker may register triggers for. When ``None``, all types are allowed.",
    )
    allow_trigger_type_registration: bool = Field(
        default=False,
        description="Whether the worker may register new trigger types. Defaults to ``False``.",
    )
    allow_function_registration: bool = Field(
        default=True,
        description="Whether the worker may register new functions. Defaults to ``True``.",
    )
    function_registration_prefix: str | None = Field(
        default=None,
        description="Optional prefix applied to all function IDs registered by this worker.",
    )
    context: dict[str, Any] = Field(
        default_factory=dict,
        description="Arbitrary context forwarded to the middleware function on every invocation.",
    )


class OnTriggerTypeRegistrationInput(BaseModel):
    """Input passed to the ``on_trigger_type_registration_function_id`` hook
    when a worker attempts to register a new trigger type through the RBAC port.
    Return an ``OnTriggerTypeRegistrationResult`` with the (possibly mapped)
    fields, or raise an exception to deny the registration.

    Attributes:
        trigger_type_id: ID of the trigger type being registered.
        description: Human-readable description of the trigger type.
        context: Auth context from ``AuthResult.context`` for this session.
    """

    trigger_type_id: str = Field(description="ID of the trigger type being registered.")
    description: str = Field(description="Human-readable description of the trigger type.")
    context: dict[str, Any] = Field(description="Auth context from ``AuthResult.context`` for this session.")


class OnTriggerTypeRegistrationResult(BaseModel):
    """Result returned from the ``on_trigger_type_registration_function_id`` hook.
    Omitted fields keep the original value from the registration request.

    Attributes:
        trigger_type_id: Mapped trigger type ID.
        description: Mapped description.
    """

    trigger_type_id: str | None = Field(default=None, description="Mapped trigger type ID.")
    description: str | None = Field(default=None, description="Mapped description.")


class OnTriggerRegistrationInput(BaseModel):
    """Input passed to the ``on_trigger_registration_function_id`` hook
    when a worker attempts to register a trigger through the RBAC port.
    Return an ``OnTriggerRegistrationResult`` with the (possibly mapped)
    fields, or raise an exception to deny the registration.

    Attributes:
        trigger_id: ID of the trigger being registered.
        trigger_type: Trigger type identifier.
        function_id: ID of the function this trigger is bound to.
        config: Trigger-specific configuration.
        metadata: Arbitrary metadata attached to the trigger.
        context: Auth context from ``AuthResult.context`` for this session.
    """

    trigger_id: str = Field(description="ID of the trigger being registered.")
    trigger_type: str = Field(description="Trigger type identifier.")
    function_id: str = Field(description="ID of the function this trigger is bound to.")
    config: Any = Field(default=None, description="Trigger-specific configuration.")
    metadata: dict[str, Any] | None = Field(default=None, description="Arbitrary metadata attached to the trigger.")
    namespace: str = Field(
        default="default",
        description="Namespace the trigger's target resolves in (explicit, or `default` when absent); "
        "the same id can exist in several namespaces, so the hook needs this to authorize per namespace.",
    )
    context: dict[str, Any] = Field(description="Auth context from ``AuthResult.context`` for this session.")


class OnTriggerRegistrationResult(BaseModel):
    """Result returned from the ``on_trigger_registration_function_id`` hook.
    Omitted fields keep the original value from the registration request.

    Attributes:
        trigger_id: Mapped trigger ID.
        trigger_type: Mapped trigger type.
        function_id: Mapped function ID.
        config: Mapped trigger configuration.
    """

    trigger_id: str | None = Field(default=None, description="Mapped trigger ID.")
    trigger_type: str | None = Field(default=None, description="Mapped trigger type.")
    function_id: str | None = Field(default=None, description="Mapped function ID.")
    config: Any = Field(default=None, description="Mapped trigger configuration.")


class OnFunctionRegistrationInput(BaseModel):
    """Input passed to the ``on_function_registration_function_id`` hook
    when a worker attempts to register a function through the RBAC port.
    Return an ``OnFunctionRegistrationResult`` with the (possibly mapped)
    fields, or raise an exception to deny the registration.

    Attributes:
        function_id: ID of the function being registered.
        description: Human-readable description of the function.
        metadata: Arbitrary metadata attached to the function.
        context: Auth context from ``AuthResult.context`` for this session.
    """

    function_id: str = Field(description="ID of the function being registered.")
    description: str | None = Field(default=None, description="Human-readable description of the function.")
    metadata: dict[str, Any] | None = Field(default=None, description="Arbitrary metadata attached to the function.")
    context: dict[str, Any] = Field(description="Auth context from ``AuthResult.context`` for this session.")


class OnFunctionRegistrationResult(BaseModel):
    """Result returned from the ``on_function_registration_function_id`` hook.
    Omitted fields keep the original value from the registration request.

    Attributes:
        function_id: Mapped function ID.
        description: Mapped description.
        metadata: Mapped metadata.
    """

    function_id: str | None = Field(default=None, description="Mapped function ID.")
    description: str | None = Field(default=None, description="Mapped description.")
    metadata: dict[str, Any] | None = Field(default=None, description="Mapped metadata.")
