"""Trigger helper constructors."""

from __future__ import annotations

import warnings
from typing import Any

from .types import ApiRouteMethod, ApiTrigger, CronTrigger, QueueTrigger, StateTrigger, StreamTrigger, TriggerCondition


def http(
    method: ApiRouteMethod,
    path: str,
    *,
    body_schema: Any | None = None,
    response_schema: dict[int, Any] | None = None,
    query_params: list[Any] | None = None,
    middleware: list[Any] | None = None,
    condition: Any | None = None,
) -> ApiTrigger:
    """Create an HTTP trigger configuration."""
    return ApiTrigger(
        path=path,
        method=method,
        condition=condition,
        body_schema=body_schema,
        response_schema=response_schema,
        query_params=query_params,
        middleware=middleware,
    )


def api(
    method: ApiRouteMethod,
    path: str,
    *,
    body_schema: Any | None = None,
    response_schema: dict[int, Any] | None = None,
    query_params: list[Any] | None = None,
    middleware: list[Any] | None = None,
    condition: Any | None = None,
) -> ApiTrigger:
    warnings.warn("api() is deprecated, use http() instead", DeprecationWarning, stacklevel=2)
    return http(
        method=method,
        path=path,
        body_schema=body_schema,
        response_schema=response_schema,
        query_params=query_params,
        middleware=middleware,
        condition=condition,
    )


def queue(
    topic: str,
    *,
    input: Any | None = None,
    config: Any | None = None,
    condition: Any | None = None,
) -> QueueTrigger:
    """Create a queue trigger configuration."""
    return QueueTrigger(
        topic=topic,
        condition=condition,
        input=input,
        config=config,
    )


def cron(expression: str, *, condition: Any | None = None) -> CronTrigger:
    """Create a cron trigger configuration."""
    return CronTrigger(expression=expression, condition=condition)


def state(*, condition: TriggerCondition | None = None) -> StateTrigger:
    """Create a state trigger."""
    return StateTrigger(type="state", condition=condition)


def stream(
    stream_name: str,
    *,
    group_id: str | None = None,
    item_id: str | None = None,
    condition: TriggerCondition | None = None,
) -> StreamTrigger:
    """Create a stream trigger."""
    return StreamTrigger(
        type="stream",
        stream_name=stream_name,
        group_id=group_id,
        item_id=item_id,
        condition=condition,
    )
