"""iii http helpers."""

from __future__ import annotations

from typing import TYPE_CHECKING, Any, Awaitable, Callable, Generic, Literal, TypeVar

from pydantic import BaseModel, ConfigDict, Field

if TYPE_CHECKING:
    from iii.types import StreamRequest, StreamResponse

TInput = TypeVar("TInput")
TOutput = TypeVar("TOutput")

HttpMethod = Literal["GET", "POST", "PUT", "PATCH", "DELETE"]
"""HTTP method accepted by :data:`HttpInvocationConfig`. Distinct from the core
``builtin_triggers`` HTTP method enum, which also covers HEAD/OPTIONS."""


class HttpAuthHmac(BaseModel):
    """HMAC signature verification using a shared secret."""

    type: Literal["hmac"] = "hmac"
    secret_key: str = Field(description="Environment variable name containing the HMAC shared secret.")


class HttpAuthBearer(BaseModel):
    """Bearer token authentication."""

    type: Literal["bearer"] = "bearer"
    token_key: str = Field(description="Environment variable name containing the bearer token.")


class HttpAuthApiKey(BaseModel):
    """API key sent via a custom header."""

    type: Literal["api_key"] = "api_key"
    header: str = Field(description="HTTP header name for the API key.")
    value_key: str = Field(description="Environment variable name containing the API key value.")


HttpAuthConfig = HttpAuthHmac | HttpAuthBearer | HttpAuthApiKey
"""Authentication configuration for HTTP-invoked functions."""


class HttpInvocationConfig(BaseModel):
    """Config for HTTP external function invocation.

    Attributes:
        url: Target URL for the HTTP invocation.
        method: HTTP method. Defaults to ``'POST'``.
        timeout_ms: Request timeout in milliseconds.
        headers: Additional HTTP headers to include in the request.
        auth: Authentication configuration (bearer, HMAC, or API key).
    """

    url: str = Field(description="Target URL for the HTTP invocation.")
    method: HttpMethod = Field(default="POST", description="HTTP method. Defaults to ``'POST'``.")
    timeout_ms: int | None = Field(default=None, description="Request timeout in milliseconds.")
    headers: dict[str, str] | None = Field(
        default=None,
        description="Additional HTTP headers to include in the request.",
    )
    auth: HttpAuthConfig | None = Field(
        default=None,
        description="Authentication configuration (bearer, HMAC, or API key).",
    )


class HttpRequest(BaseModel, Generic[TInput]):
    """Represents a buffered HTTP request."""

    path_params: dict[str, str] = Field(default_factory=dict)
    query_params: dict[str, str | list[str]] = Field(default_factory=dict)
    body: Any | None = None
    headers: dict[str, str | list[str]] = Field(default_factory=dict)
    method: str = "GET"


class HttpResponse(BaseModel, Generic[TOutput]):
    """Represents a buffered HTTP response."""

    model_config = ConfigDict(populate_by_name=True, arbitrary_types_allowed=True)

    status_code: int = Field(alias="statusCode")
    body: Any | None = None
    headers: dict[str, str] = Field(default_factory=dict)


def http(
    callback: Callable[[StreamRequest, StreamResponse], Awaitable[HttpResponse[Any] | None]],
) -> Callable[[Any], Awaitable[HttpResponse[Any] | None]]:
    """Wrap a streaming handler so it receives typed StreamRequest and StreamResponse.

    Takes a callback ``(req, res) -> HttpResponse | None`` and returns a
    function the iii engine can invoke directly.  The wrapper converts the
    raw dict (or ``InternalHttpRequest``) delivered by the engine into the
    typed ``StreamRequest`` / ``StreamResponse`` pair that the callback expects.
    """
    from iii.types import InternalHttpRequest, StreamRequest, StreamResponse

    async def wrapper(req: Any) -> HttpResponse[Any] | None:
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

        http_response = StreamResponse(internal.response)
        http_request = StreamRequest(
            path_params=internal.path_params,
            query_params=internal.query_params,
            body=internal.body,
            headers=internal.headers,
            method=internal.method,
            request_body=internal.request_body,
        )
        return await callback(http_request, http_response)

    return wrapper


__all__ = [
    "HttpAuthApiKey",
    "HttpAuthBearer",
    "HttpAuthConfig",
    "HttpAuthHmac",
    "HttpInvocationConfig",
    "HttpMethod",
    "HttpRequest",
    "HttpResponse",
    "http",
]
