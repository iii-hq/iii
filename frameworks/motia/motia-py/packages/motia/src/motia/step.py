"""Step builder for defining Motia steps."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Awaitable, Callable

from .types import StepConfig

StepHandler = Callable[..., Awaitable[Any]]


@dataclass
class StepDefinition:
    """A complete step definition with config and handler."""

    config: StepConfig
    handler: StepHandler


class StepBuilder:
    """Builder for creating a StepDefinition."""

    def __init__(self, config: StepConfig) -> None:
        self.config = config

    def handle(self, handler: StepHandler) -> StepDefinition:
        """Attach a handler to complete the step definition."""
        return StepDefinition(config=self.config, handler=handler)


def step(
    config: StepConfig | dict[str, Any],
    handler: StepHandler | None = None,
) -> StepDefinition | StepBuilder:
    """Create a step definition or builder.

    Usage:
        # With handler (returns StepDefinition):
        my_step = step(config, handler)

        # Without handler (returns StepBuilder):
        my_step = step(config).handle(handler)

    Args:
        config: Step configuration dict or StepConfig instance.
        handler: Optional async handler function.

    Returns:
        StepDefinition if handler provided, StepBuilder otherwise.
    """
    if not isinstance(config, StepConfig):
        config = StepConfig.model_validate(config)

    if handler is not None:
        return StepDefinition(config=config, handler=handler)
    return StepBuilder(config=config)
