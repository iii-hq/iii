"""Trigger types and handlers."""

from __future__ import annotations

from abc import ABC, abstractmethod
from typing import TYPE_CHECKING, Any, Awaitable, Callable, Generic, TypeVar

from pydantic import BaseModel, ConfigDict

if TYPE_CHECKING:
    from .iii import III

TConfig = TypeVar("TConfig")
C = TypeVar("C")
R = TypeVar("R")


class TriggerConfig(BaseModel, Generic[TConfig]):
    """Configuration passed to a trigger handler when a trigger instance is
    registered or unregistered.

    Attributes:
        id: Trigger instance ID.
        function_id: Function to invoke when the trigger fires.
        config: Trigger-specific configuration.
        metadata: Arbitrary user-specifiable metadata supplied to the triggered
            handler function on every invocation.
        namespace: Resolved namespace the target function uses. Current SDKs
            fill an omitted registration value from the registering worker's
            namespace; ``None`` is the legacy/default case.
    """

    model_config = ConfigDict(arbitrary_types_allowed=True)

    id: str
    function_id: str
    config: Any  # TConfig
    metadata: dict[str, Any] | None = None
    # A provider that stores this config and later calls trigger() must pass the
    # resolved namespace through.
    namespace: str | None = None


class TriggerHandler(ABC, Generic[TConfig]):
    """Abstract base class for trigger handlers."""

    @abstractmethod
    async def register_trigger(self, config: TriggerConfig[TConfig]) -> None:
        """Register a trigger with the given configuration."""
        pass

    @abstractmethod
    async def unregister_trigger(self, config: TriggerConfig[TConfig]) -> None:
        """Unregister a trigger with the given configuration."""
        pass


class Trigger:
    """Represents a registered trigger."""

    def __init__(
        self,
        unregister_fn: Any,
        registration_error_fn: Callable[[], dict[str, Any] | None] | None = None,
    ) -> None:
        self._unregister_fn = unregister_fn
        self._registration_error_fn = registration_error_fn

    def unregister(self) -> None:
        """Unregister this trigger."""
        self._unregister_fn()

    @property
    def registration_error(self) -> dict[str, Any] | None:
        """The engine's rejection of this binding, or ``None``.

        Registration is asynchronous and only failures are acked, so ``None``
        means "no failure reported yet", not "confirmed live".

        The common cause is ``trigger_type_not_found`` from a boot-order race:
        the binding was requested before the provider registered the trigger
        type. A reconnect re-sends the registration and clears this.

        To confirm a binding IS live, call ``engine::registered-triggers::list``
        with ``trigger_type``, ``function_id``, and ``namespace``.
        """
        if self._registration_error_fn is None:
            return None
        return self._registration_error_fn()


class TriggerTypeRef(Generic[C, R]):
    """Typed handle returned by :meth:`iii.III.register_trigger_type`.

    Type parameters:

    - ``C``: configuration type for :meth:`register_trigger`
    - ``R``: call-request type for :meth:`register_function`

    Example::

        webhook = worker.register_trigger_type(
            RegisterTriggerTypeInput(
                id="webhook",
                description="Incoming webhook trigger",
                trigger_request_format=WebhookTriggerConfig,
                call_request_format=WebhookCallRequest,
            ),
            WebhookHandler(),
        )

        # Typed: config must be WebhookTriggerConfig
        webhook.register_trigger("my::handler", WebhookTriggerConfig(url="/hook"))

        # Typed: handler receives WebhookCallRequest
        webhook.register_function("my::handler", handle_webhook)
    """

    def __init__(
        self,
        iii: "III",
        trigger_type_id: str,
        config_cls: type[C] | None = None,
        request_cls: type[R] | None = None,
    ) -> None:
        self._iii = iii
        self._trigger_type_id = trigger_type_id
        self._config_cls = config_cls
        self._request_cls = request_cls

    def register_trigger(
        self, function_id: str, config: C, metadata: dict[str, Any] | None = None
    ) -> Trigger:
        """Register a trigger with validated config.

        If the config is a Pydantic model it is serialized automatically.
        """
        if hasattr(config, "model_dump"):
            config_value = config.model_dump()
        else:
            config_value = config

        # Pairs a function with its trigger, so it defaults the trigger's
        # namespace to this worker's — otherwise the function lands in the
        # worker's namespace and the trigger in `default`, never resolving it.
        # The low-level `register_trigger` keeps the engine default (`default`).
        return self._iii.register_trigger(
            {
                "type": self._trigger_type_id,
                "function_id": function_id,
                "config": config_value,
                "metadata": metadata,
                "namespace": self._iii._worker_namespace(),
            }
        )

    def register_function(
        self,
        function_id: str,
        handler: Callable[[R], Any] | Callable[[R], Awaitable[Any]],
        *,
        description: str | None = None,
    ) -> Any:
        """Register a function whose input matches the call-request format."""
        return self._iii.register_function(
            function_id,
            handler,
            description=description,
        )
