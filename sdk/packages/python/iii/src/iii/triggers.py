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
    """Configuration for a trigger."""

    model_config = ConfigDict(arbitrary_types_allowed=True)

    id: str
    function_id: str
    config: Any  # TConfig


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

    def __init__(self, unregister_fn: Any) -> None:
        self._unregister_fn = unregister_fn

    def unregister(self) -> None:
        """Unregister this trigger."""
        self._unregister_fn()


class TriggerTypeRef(Generic[C, R]):
    """Typed handle returned by :meth:`III.register_trigger_type`.

    Type parameters:

    - ``C`` — configuration type for :meth:`register_trigger`
    - ``R`` — call-request type for :meth:`register_function`

    Example::

        webhook = iii.register_trigger_type(
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

    def register_trigger(self, function_id: str, config: C) -> Trigger:
        """Register a trigger with validated config.

        If the config is a Pydantic model it is serialized automatically.
        """
        if hasattr(config, "model_dump"):
            config_value = config.model_dump()
        else:
            config_value = config

        return self._iii.register_trigger(
            {
                "type": self._trigger_type_id,
                "function_id": function_id,
                "config": config_value,
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
