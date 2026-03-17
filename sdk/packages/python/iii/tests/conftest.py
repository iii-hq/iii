"""Shared fixtures for III SDK integration tests."""

import os
import time

import pytest

from iii.iii import III

ENGINE_WS_URL = os.environ.get("III_URL", "ws://localhost:49199")
ENGINE_HTTP_URL = os.environ.get("III_HTTP_URL", "http://localhost:3199")


@pytest.fixture
def iii_client():
    """Create and connect an III client, shut it down after the test."""
    client = III(ENGINE_WS_URL)
    client._wait_until_connected()  # wait for auto-connect to complete
    time.sleep(0.3)
    yield client
    client.shutdown()


@pytest.fixture
def engine_http_url():
    return ENGINE_HTTP_URL
