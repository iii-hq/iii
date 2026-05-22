"""WebSocket reconnection configuration for iii observability connections."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass
class ReconnectionConfig:
    """Configuration for WebSocket reconnection behavior.

    Attributes:
        initial_delay_ms: Starting delay in milliseconds. Default ``1000``.
        max_delay_ms: Maximum delay cap in milliseconds. Default ``30000``.
        backoff_multiplier: Exponential backoff multiplier. Default ``2.0``.
        jitter_factor: Random jitter factor (0--1). Default ``0.3``.
        max_retries: Maximum retry attempts. ``-1`` for infinite. Default ``-1``.
    """

    initial_delay_ms: int = 1000
    max_delay_ms: int = 30000
    backoff_multiplier: float = 2.0
    jitter_factor: float = 0.3
    max_retries: int = -1
