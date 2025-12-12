"""Trigger types and handlers."""

from abc import ABC, abstractmethod
from typing import Any, Generic, TypeVar

from pydantic import BaseModel, ConfigDict

TConfig = TypeVar("TConfig")


class TriggerConfig(BaseModel, Generic[TConfig]):
    """Configuration for a trigger."""

    model_config = ConfigDict(arbitrary_types_allowed=True)

    id: str
    function_path: str
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
