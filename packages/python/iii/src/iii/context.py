"""Context management for the III SDK."""

from contextvars import ContextVar
from typing import Any, Awaitable, Callable, TypeVar

from pydantic import BaseModel, ConfigDict

from .logger import Logger

T = TypeVar("T")


class Context(BaseModel):
    """Execution context containing logger and other utilities."""

    model_config = ConfigDict(arbitrary_types_allowed=True)

    logger: Logger


_context_var: ContextVar[Context | None] = ContextVar("iii_context", default=None)


async def with_context(fn: Callable[[Context], Awaitable[T]], context: Context) -> T:
    """Execute a function within a context."""
    token = _context_var.set(context)
    try:
        return await fn(context)
    finally:
        _context_var.reset(token)


def get_context() -> Context:
    """Get the current execution context."""
    ctx = _context_var.get()
    if ctx is not None:
        return ctx

    # Return a default context if none is set
    logger = Logger()
    return Context(logger=logger)
