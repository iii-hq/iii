"""Tests for process hold-alive behavior."""

from iii.iii import III


def test_background_thread_is_not_daemon(monkeypatch) -> None:
    """The background event-loop thread must be non-daemon so it keeps the process alive."""
    monkeypatch.setattr("iii.telemetry.init_otel", lambda **kwargs: None)
    monkeypatch.setattr("iii.telemetry.attach_event_loop", lambda loop: None)

    async def fake_do_connect(self: III) -> None:
        return None

    monkeypatch.setattr(III, "_do_connect", fake_do_connect)

    client = III("ws://fake")
    try:
        assert client._thread.is_alive()
        assert not client._thread.daemon, "Thread must be non-daemon to keep process alive"
    finally:
        client.shutdown()


def test_shutdown_stops_background_thread(monkeypatch) -> None:
    """After shutdown(), the background thread should stop within a reasonable timeout."""
    monkeypatch.setattr("iii.telemetry.init_otel", lambda **kwargs: None)
    monkeypatch.setattr("iii.telemetry.attach_event_loop", lambda loop: None)

    async def fake_do_connect(self: III) -> None:
        return None

    monkeypatch.setattr(III, "_do_connect", fake_do_connect)

    client = III("ws://fake")
    assert client._thread.is_alive()

    client.shutdown()

    # Thread should stop after shutdown
    client._thread.join(timeout=3)
    assert not client._thread.is_alive(), "Thread should have stopped after shutdown()"


def test_shutdown_async_stops_background_thread(monkeypatch) -> None:
    """After shutdown_async(), the background thread should also stop."""
    monkeypatch.setattr("iii.telemetry.init_otel", lambda **kwargs: None)
    monkeypatch.setattr("iii.telemetry.attach_event_loop", lambda loop: None)

    async def fake_do_connect(self: III) -> None:
        return None

    monkeypatch.setattr(III, "_do_connect", fake_do_connect)

    client = III("ws://fake")
    assert client._thread.is_alive()

    # Call shutdown_async on the client's own event loop
    client._run_on_loop(client.shutdown_async())

    # The loop should stop after shutdown_async schedules loop.stop()
    client._thread.join(timeout=3)
    assert not client._thread.is_alive(), "Thread should have stopped after shutdown_async()"
