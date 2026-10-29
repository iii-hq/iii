"""iii worker connection manager helpers."""

from typing import Any

from pydantic import BaseModel, Field

__all__ = [
    "AuthInput",
    "AuthResult",
    "OnFunctionRegistrationInput",
    "OnFunctionRegistrationResult",
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
        namespaces: Grants scoped to one namespace each, e.g. ``{"orders": ["svc::*"]}``.
            The keys are also the namespaces the session may declare on
            ``engine::workers::register``. Leave empty to add no namespace-scoped
            grants. The session may declare any namespace, and only ``allowed_functions``
            and ``expose_functions`` apply.
        allowed_functions: Additional function IDs to allow beyond ``expose_functions``,
            in the ``default`` namespace only. Put per-namespace grants in ``namespaces``.
        forbidden_functions: Function IDs to deny even if they match ``expose_functions``.
        allow_function_registration: Whether the worker may register new functions. Defaults to ``True`` if omitted.
        function_registration_prefix: Optional prefix applied to all function IDs registered
            by this worker.
        context: Arbitrary context forwarded to the middleware function on every invocation.
    """

    namespaces: dict[str, list[str]] = Field(
        default_factory=dict,
        description=(
            'Grants scoped to one namespace each, e.g. ``{"orders": ["svc::*"]}``. A value is an '
            'exact function ID or a wildcard, bare or in the ``match("...")`` spelling. The keys '
            "are also the namespaces the session may declare on ``engine::workers::register``. "
            "Leave empty to add no namespace-scoped grants: the session may declare any namespace, "
            "and only ``allowed_functions`` and ``expose_functions`` apply."
        ),
    )
    allowed_functions: list[str] = Field(
        default_factory=list,
        description=(
            "Additional function IDs to allow beyond ``expose_functions``, in the ``default`` "
            "namespace only. Put per-namespace grants in ``namespaces``."
        ),
    )
    forbidden_functions: list[str] = Field(
        default_factory=list,
        description="Function IDs to deny even if they match ``expose_functions``.",
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


class OnFunctionRegistrationInput(BaseModel):
    """Input passed to the ``on_function_registration_function_id`` hook
    when a worker attempts to register a function through the RBAC port.
    Return an ``OnFunctionRegistrationResult`` with the (possibly mapped)
    fields, or raise an exception to deny the registration.

    Attributes:
        function_id: ID of the function being registered.
        description: Human-readable description of the function.
        metadata: Arbitrary metadata attached to the function.
        namespace: Namespace the function registers in. It is the explicit
            registration value, or ``default`` when the registration omitted it.
        private: Whether only the registering worker may call the function.
        observable: Whether other workers may observe the function.
        args_format: JSON Schema of the observers' ``args``, when the registration sets one.
        context: Auth context from ``AuthResult.context`` for this session.
    """

    function_id: str = Field(description="ID of the function being registered.")
    description: str | None = Field(default=None, description="Human-readable description of the function.")
    metadata: dict[str, Any] | None = Field(default=None, description="Arbitrary metadata attached to the function.")
    namespace: str = Field(
        default="default",
        description="Namespace the function registers in; the same id can exist in several namespaces, "
        "so the hook needs this to authorize per namespace.",
    )
    private: bool = Field(default=False, description="Whether only the registering worker may call the function.")
    observable: bool = Field(default=True, description="Whether other workers may observe the function.")
    args_format: Any = Field(
        default=None, description="JSON Schema of the observers' ``args``, when the registration sets one."
    )
    context: dict[str, Any] = Field(description="Auth context from ``AuthResult.context`` for this session.")


class OnFunctionRegistrationResult(BaseModel):
    """Result returned from the ``on_function_registration_function_id`` hook.
    Omitted fields keep the original value from the registration request:
    return ``model_dump(exclude_none=True)``.

    Attributes:
        function_id: Mapped function ID.
        description: Mapped description.
        metadata: Mapped metadata.
        private: Replaces the registration's ``private``.
        observable: Replaces the registration's ``observable``.
        args_format: Replaces the registration's ``args_format``.
    """

    function_id: str | None = Field(default=None, description="Mapped function ID.")
    description: str | None = Field(default=None, description="Mapped description.")
    metadata: dict[str, Any] | None = Field(default=None, description="Mapped metadata.")
    private: bool | None = Field(default=None, description="Replaces the registration's ``private``.")
    observable: bool | None = Field(default=None, description="Replaces the registration's ``observable``.")
    args_format: Any = Field(default=None, description="Replaces the registration's ``args_format``.")
