"""Shared fixtures for III SDK integration tests."""

import os
import time

import pytest

# Fixtures of the observe tests: a fake engine for the unit tests, and workers
# of their own against the real engine for the integration tests.
from observe_e2e_utils import fid, worker  # noqa: F401
from observe_fake_engine import connect, engine, engine_factory, quiet_otel  # noqa: F401

from iii import InitOptions
from iii.iii import III

ENGINE_WS_URL = os.environ.get("III_URL", "ws://localhost:49199")
ENGINE_HTTP_URL = os.environ.get("III_HTTP_URL", "http://localhost:3199")


@pytest.fixture
def iii_client():
    """Create and connect an III client, shut it down after the test."""
    # Explicit name: the engine allows one live worker per (namespace, name), and
    # the default name is shared by every worker in this single-process run.
    client = III(ENGINE_WS_URL, InitOptions(worker_name="iii-test-client"))
    client._wait_until_connected()  # wait for auto-connect to complete
    time.sleep(0.3)
    yield client
    client.shutdown()


@pytest.fixture
def engine_http_url():
    return ENGINE_HTTP_URL
