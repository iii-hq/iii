"""Tests for the auto-installed SIGTERM/SIGINT handlers in III.__init__.

Background: the in-VM supervisor (iii-worker) cycles dev-mode workers by
sending SIGTERM. Without a handler, Python takes its default termination
action and the WebSocket dies without a Close frame — which leaves the
engine overwriting stale registrations on each restart. These tests verify
the handler is installed by default, can be opted out, is cleaned up on
shutdown, and actually calls shutdown() + os._exit on delivery.
"""

from __future__ import annotations

import signal
import threading
from typing import Any
from unittest import mock

from iii import InitOptions
from iii.iii import III


def _fake_do_connect_factory() -> Any:
    """Return a no-op replacement for III._do_connect so tests don't open
    real sockets. Connection state is still transitioned by connect_async
    but no network happens.
    """

    async def fake_do_connect(self: III) -> None:
        return None

    return fake_do_connect


def _build_client(monkeypatch: Any, **init_kwargs: Any) -> III:
    """Construct an III instance with networking stubbed out. Tests are
    pure — they only care about signal wiring, not wire protocol.
    """
    monkeypatch.setattr(III, "_do_connect", _fake_do_connect_factory())

    # Stub OTel to avoid background metric exporters + their own sockets.
    import iii.telemetry as telemetry

    monkeypatch.setattr(telemetry, "init_otel", lambda **_kw: None)
    monkeypatch.setattr(telemetry, "attach_event_loop", lambda *_a, **_kw: None)

    async def fake_shutdown_otel_async() -> None:
        return None

    monkeypatch.setattr(telemetry, "shutdown_otel_async", fake_shutdown_otel_async)

    return III("ws://127.0.0.1:1/never-binds", options=InitOptions(**init_kwargs))


class TestAutoShutdown:
    def test_installs_sigterm_and_sigint_handlers_by_default(
        self, monkeypatch: Any
    ) -> None:
        """Default init path should replace the SIGTERM and SIGINT handlers
        with our shutdown signal handler.
        """
        if threading.current_thread() is not threading.main_thread():
            # signal.signal only works on the main thread in CPython; skip in
            # weird environments that host pytest on a worker thread.
            return

        client = _build_client(monkeypatch)
        try:
            assert (
                signal.getsignal(signal.SIGTERM) == client._on_shutdown_signal
            ), "SIGTERM handler should be installed by default"
            assert (
                signal.getsignal(signal.SIGINT) == client._on_shutdown_signal
            ), "SIGINT handler should be installed by default"
        finally:
            # shutdown restores previous handlers so subsequent tests aren't
            # affected.
            client.shutdown()

    def test_does_not_install_handlers_when_auto_shutdown_is_false(
        self, monkeypatch: Any
    ) -> None:
        before_term = signal.getsignal(signal.SIGTERM)
        before_int = signal.getsignal(signal.SIGINT)

        client = _build_client(monkeypatch, auto_shutdown=False)
        try:
            assert signal.getsignal(signal.SIGTERM) == before_term
            assert signal.getsignal(signal.SIGINT) == before_int
        finally:
            client.shutdown()

    def test_shutdown_restores_previous_signal_handlers(
        self, monkeypatch: Any
    ) -> None:
        """Explicit shutdown() must restore the handlers that were in place
        before the SDK installed its own. Otherwise a worker that cleans up
        cleanly would still exit the process on the next signal — surprising
        for anyone who later wires their own handler.
        """
        if threading.current_thread() is not threading.main_thread():
            return

        sentinel_term = signal.signal(signal.SIGTERM, signal.SIG_DFL)
        sentinel_int = signal.signal(signal.SIGINT, signal.SIG_DFL)
        try:
            client = _build_client(monkeypatch)
            # SDK installs its handler...
            assert signal.getsignal(signal.SIGTERM) == client._on_shutdown_signal
            assert signal.getsignal(signal.SIGINT) == client._on_shutdown_signal

            client.shutdown()

            # ...and shutdown restores what was there before.
            assert signal.getsignal(signal.SIGTERM) == signal.SIG_DFL
            assert signal.getsignal(signal.SIGINT) == signal.SIG_DFL
        finally:
            # Restore original so the test suite doesn't leak state.
            signal.signal(signal.SIGTERM, sentinel_term)
            signal.signal(signal.SIGINT, sentinel_int)

    def test_signal_handler_calls_shutdown_then_exits(self) -> None:
        """Delivering SIGTERM through our handler should call shutdown() and
        then os._exit with the conventional 128 + signum code.

        Exercised on a duck-typed stand-in rather than a real III instance.
        A real III spins up a non-daemon background event-loop thread that
        a mocked shutdown() would never tear down, and would keep pytest
        alive forever. The handler only touches ``self.shutdown`` and the
        module-level ``os._exit``, so a SimpleNamespace exercises it
        faithfully.
        """
        from types import SimpleNamespace

        shutdown_mock = mock.MagicMock()
        stand_in = SimpleNamespace(shutdown=shutdown_mock)

        with mock.patch("iii.iii.os._exit") as exit_mock:
            III._on_shutdown_signal(stand_in, signal.SIGTERM, None)  # type: ignore[arg-type]
            shutdown_mock.assert_called_once()
            exit_mock.assert_called_once_with(128 + int(signal.SIGTERM))

    def test_signal_handler_exits_even_when_shutdown_raises(self) -> None:
        """If shutdown() blows up, we still want os._exit to fire — otherwise
        a misbehaving shutdown path would leave the worker process hanging
        on SIGTERM, defeating the whole point of auto-shutdown in dev.
        """
        from types import SimpleNamespace

        shutdown_mock = mock.MagicMock(side_effect=RuntimeError("boom"))
        stand_in = SimpleNamespace(shutdown=shutdown_mock)

        with mock.patch("iii.iii.os._exit") as exit_mock:
            III._on_shutdown_signal(stand_in, signal.SIGINT, None)  # type: ignore[arg-type]
            exit_mock.assert_called_once_with(128 + int(signal.SIGINT))
