"""HTTP client auto-instrumentation for the iii Python SDK.

Mirrors the Rust execute_traced_request shape: wraps an httpx Request in an
OTel CLIENT span with HTTP semantic-convention attributes, injects W3C
traceparent into outgoing headers, and records exceptions on network errors.
"""

from __future__ import annotations

import os

import httpx
from opentelemetry import trace
from opentelemetry.propagate import inject
from opentelemetry.trace import SpanKind, Status, StatusCode

_SAFE_REQUEST_HEADERS = ("content-type", "accept")
_SAFE_RESPONSE_HEADERS = ("content-type",)


def _span_name(method: str, path: str | None) -> str:
    return f"{method} {path}" if path else method


def _fetch_ignore_url_patterns() -> list[str]:
    """Substring patterns from OTEL_FETCH_IGNORE_URLS (comma-separated)."""
    return [
        s.strip()
        for s in (os.environ.get("OTEL_FETCH_IGNORE_URLS") or "").split(",")
        if s.strip()
    ]


def _should_ignore_fetch_url(url: str) -> bool:
    return any(pattern in url for pattern in _fetch_ignore_url_patterns())


async def execute_traced_request(
    client: httpx.AsyncClient,
    request: httpx.Request,
) -> httpx.Response:
    """Execute an httpx Request inside an OTel CLIENT span.

    - Injects W3C traceparent into outgoing request headers.
    - Records HTTP semantic-convention attributes on the span.
    - Sets ERROR span status for responses with status >= 400.
    - Records exceptions for network-level errors.

    Args:
        client: httpx async client used to send the request.
        request: The httpx request to execute.
    """
    url = request.url
    url_str = str(url)
    if _should_ignore_fetch_url(url_str):
        return await client.send(request)

    method = request.method.upper()
    path = url.path or None
    query = url.query
    query_str: str | None
    if isinstance(query, bytes):
        query_str = query.decode() if query else None
    else:
        query_str = query or None

    attributes: dict[str, str | int] = {
        "http.request.method": method,
        "url.full": str(url),
    }
    if url.host:
        attributes["server.address"] = url.host
    if url.scheme:
        attributes["url.scheme"] = url.scheme
        attributes["network.protocol.name"] = "http"
    if path:
        attributes["url.path"] = path
    if url.port:
        attributes["server.port"] = url.port
    if query_str:
        attributes["url.query"] = query_str

    tracer = trace.get_tracer("iii-python-sdk")
    name = _span_name(method, path)

    with tracer.start_as_current_span(name, kind=SpanKind.CLIENT, attributes=attributes) as span:
        carrier: dict[str, str] = {}
        inject(carrier)
        for k, v in carrier.items():
            request.headers[k] = v

        for h in _SAFE_REQUEST_HEADERS:
            v = request.headers.get(h)
            if v:
                span.set_attribute(f"http.request.header.{h}", v)
        if request.content:
            span.set_attribute("http.request.body.size", len(request.content))

        try:
            response = await client.send(request)
        except httpx.HTTPError as err:
            span.record_exception(err)
            span.set_status(Status(StatusCode.ERROR, str(err)))
            span.set_attribute("error.type", type(err).__name__)
            raise

        span.set_attribute("http.response.status_code", response.status_code)
        cl = response.headers.get("content-length")
        if cl:
            try:
                span.set_attribute("http.response.body.size", int(cl))
            except ValueError:
                pass
        for h in _SAFE_RESPONSE_HEADERS:
            v = response.headers.get(h)
            if v:
                span.set_attribute(f"http.response.header.{h}", v)

        if response.status_code >= 400:
            span.set_status(Status(StatusCode.ERROR, str(response.status_code)))
            span.set_attribute("error.type", str(response.status_code))
        else:
            span.set_status(Status(StatusCode.OK))

        return response
