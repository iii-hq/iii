"""Tests for trigger helper constructors."""

import warnings

import pytest

from motia.triggers import api, http


def test_http_returns_api_trigger():
    """http() returns ApiTrigger with method and path."""
    t = http("GET", "/users")
    assert t.type == "http"
    assert t.method == "GET"
    assert t.path == "/users"


def test_api_deprecated_delegates_to_http():
    """api() is deprecated and delegates to http()."""
    with pytest.warns(DeprecationWarning, match="api\\(\\) is deprecated"):
        t = api("GET", "/users")
    assert t == http("GET", "/users")
    assert t.type == "http"
    assert t.method == "GET"
    assert t.path == "/users"
