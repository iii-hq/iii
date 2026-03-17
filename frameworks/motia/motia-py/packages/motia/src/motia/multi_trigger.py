"""Multi-trigger step builder for Motia steps with multiple trigger types."""

from __future__ import annotations

from typing import Any, Awaitable, Callable

from .step import StepDefinition, StepHandler
from .types import StepConfig

TriggerHandlers = dict[str, Callable[..., Awaitable[Any]]]


class MultiTriggerStepBuilder:
    """Builder for steps with multiple trigger types.

    Supports both dict-style and chainable handler registration:

        # Dict style:
        multi_trigger_step(config).handlers({
            "queue": queue_handler,
            "http": http_handler,
        })

        # Chainable style:
        multi_trigger_step(config)
            .on_queue(queue_handler)
            .on_http(http_handler)
            .handlers()
    """

    def __init__(self, config: StepConfig) -> None:
        self.config = config
        self._handlers: TriggerHandlers = {}

    def _create_unified_handler(self) -> StepHandler:
        """Create a unified handler that dispatches based on trigger type."""
        handlers = dict(self._handlers)

        async def unified_handler(input_data: Any, ctx: Any) -> Any:
            trigger_type = ctx.trigger.type
            if trigger_type == "queue" and "queue" in handlers:
                return await handlers["queue"](input_data, ctx)
            if trigger_type == "http" and "http" in handlers:
                return await handlers["http"](input_data, ctx)
            if trigger_type == "cron" and "cron" in handlers:
                return await handlers["cron"](ctx)
            if trigger_type == "state" and "state" in handlers:
                return await handlers["state"](input_data, ctx)
            if trigger_type == "stream" and "stream" in handlers:
                return await handlers["stream"](input_data, ctx)

            raise RuntimeError(
                f"No handler defined for trigger type: {trigger_type}. Available handlers: {', '.join(handlers.keys())}"
            )

        return unified_handler

    def on_queue(self, handler: Callable[..., Awaitable[Any]]) -> MultiTriggerStepBuilder:
        """Register a handler for queue triggers."""
        self._handlers["queue"] = handler
        return self

    def on_http(self, handler: Callable[..., Awaitable[Any]]) -> MultiTriggerStepBuilder:
        """Register a handler for HTTP triggers."""
        self._handlers["http"] = handler
        return self

    def on_cron(self, handler: Callable[..., Awaitable[Any]]) -> MultiTriggerStepBuilder:
        """Register a handler for cron triggers."""
        self._handlers["cron"] = handler
        return self

    def on_state(self, handler: Callable[..., Awaitable[Any]]) -> MultiTriggerStepBuilder:
        """Register a handler for state triggers."""
        self._handlers["state"] = handler
        return self

    def on_stream(self, handler: Callable[..., Awaitable[Any]]) -> MultiTriggerStepBuilder:
        """Register a handler for stream triggers."""
        self._handlers["stream"] = handler
        return self

    def handlers(self, handlers: TriggerHandlers | None = None) -> StepDefinition:
        """Finalize the builder and create a StepDefinition.

        Args:
            handlers: Optional dict of trigger type -> handler mappings.
                     If provided, merges with any previously registered handlers.
                     If omitted, uses only the handlers set via on_*() methods.

        Returns:
            StepDefinition with the unified handler.
        """
        if handlers:
            self._handlers.update(handlers)
        return StepDefinition(config=self.config, handler=self._create_unified_handler())


def multi_trigger_step(
    config: StepConfig | dict[str, Any],
) -> MultiTriggerStepBuilder:
    """Create a multi-trigger step builder.

    Usage:
        # Dict-style handlers:
        my_step = multi_trigger_step(config).handlers({
            "queue": queue_handler,
            "http": http_handler,
        })

        # Chainable handlers:
        my_step = (
            multi_trigger_step(config)
            .on_queue(queue_handler)
            .on_http(http_handler)
            .handlers()
        )

    Args:
        config: Step configuration dict or StepConfig instance.

    Returns:
        MultiTriggerStepBuilder for registering handlers.
    """
    if not isinstance(config, StepConfig):
        config = StepConfig.model_validate(config)
    return MultiTriggerStepBuilder(config)
