"""Utility functions for the III SDK."""

from __future__ import annotations

import json
from typing import TYPE_CHECKING, Any, Awaitable, Callable

from .types import HttpRequest, HttpResponse, InternalHttpRequest
from .types import is_channel_ref as is_channel_ref  # noqa: F401 - re-exported from types

if TYPE_CHECKING:
    from .types import ApiResponse


def http(
    callback: Callable[[HttpRequest, HttpResponse], Awaitable[ApiResponse[Any] | None]],
) -> Callable[[Any], Awaitable[ApiResponse[Any] | None]]:
    """Wrap an HTTP handler so it receives typed HttpRequest and HttpResponse.

    Takes a callback ``(req, res) -> ApiResponse | None`` and returns a
    function the iii engine can invoke directly.  The wrapper converts the
    raw dict (or ``InternalHttpRequest``) delivered by the engine into the
    typed ``HttpRequest`` / ``HttpResponse`` pair that the callback expects.
    """

    async def wrapper(req: Any) -> ApiResponse[Any] | None:
        if isinstance(req, InternalHttpRequest):
            internal = req
        elif isinstance(req, dict):
            internal = InternalHttpRequest(
                path_params=req.get("path_params", {}),
                query_params=req.get("query_params", {}),
                body=req.get("body"),
                headers=req.get("headers", {}),
                method=req.get("method", "GET"),
                response=req["response"],
                request_body=req["request_body"],
            )
        else:
            internal = req

        http_response = HttpResponse(internal.response)
        http_request = HttpRequest(
            path_params=internal.path_params,
            query_params=internal.query_params,
            body=internal.body,
            headers=internal.headers,
            method=internal.method,
            request_body=internal.request_body,
        )
        return await callback(http_request, http_response)

    return wrapper


def safe_stringify(value: Any) -> str:
    """Safely stringify a value, handling circular references and non-serializable types."""
    try:
        return json.dumps(value, default=str)
    except (TypeError, ValueError):
        return "[unserializable]"
