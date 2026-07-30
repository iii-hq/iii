"""Engine address resolution: explicit argument, then III_URL, then the default.

The supervisor that spawned this process -- ``iii compose``, a container
runtime, systemd -- sets ``III_URL``, the same way it sets ``III_NAMESPACE`` and
``III_WORKER_NAME``.
"""

from __future__ import annotations

import pytest

from iii import DEFAULT_ENGINE_URL
from iii.iii import resolve_engine_url


@pytest.fixture(autouse=True)
def _clear_env(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("III_URL", raising=False)


def test_default_is_the_ipv4_loopback() -> None:
    assert resolve_engine_url() == DEFAULT_ENGINE_URL
    assert DEFAULT_ENGINE_URL == "ws://127.0.0.1:49134"


def test_reads_iii_url_when_no_address_is_passed(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("III_URL", "ws://engine.example:9000")
    assert resolve_engine_url() == "ws://engine.example:9000"


def test_explicit_address_wins_over_the_environment(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("III_URL", "ws://from-env:1")
    assert resolve_engine_url("ws://explicit:2") == "ws://explicit:2"


def test_empty_iii_url_is_ignored(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("III_URL", "")
    assert resolve_engine_url() == DEFAULT_ENGINE_URL


def test_register_worker_accepts_no_address() -> None:
    """The zero-arg form must be part of the public signature."""
    import inspect

    from iii import register_worker

    signature = inspect.signature(register_worker)
    assert signature.parameters["address"].default is None
