"""Type guards for step configurations."""

from typing import Any

from .types import Step


def is_api_step(step: Step[Any]) -> bool:
    """Check if a step is an API step."""
    return step.config.type == "api"


def is_event_step(step: Step[Any]) -> bool:
    """Check if a step is an event step."""
    return step.config.type == "event"


def is_noop_step(step: Step[Any]) -> bool:
    """Check if a step is a noop step."""
    return step.config.type == "noop"


def is_cron_step(step: Step[Any]) -> bool:
    """Check if a step is a cron step."""
    return step.config.type == "cron"
