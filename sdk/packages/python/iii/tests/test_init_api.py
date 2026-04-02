import time

from iii import InitOptions, register_worker
from iii.iii import III


def test_register_worker_returns_iii_instance(monkeypatch) -> None:
    """register_worker should return an III instance with auto-connect initiated."""
    import iii.iii as iii_module

    async def fake_do_connect(self: III) -> None:
        return None

    monkeypatch.setattr("iii.telemetry.init_otel", lambda **kwargs: None)
    monkeypatch.setattr("iii.telemetry.attach_event_loop", lambda loop: None)
    monkeypatch.setattr(III, "_do_connect", fake_do_connect)

    client = register_worker("ws://fake")
    assert isinstance(client, III)

    client.shutdown()


def test_register_worker_is_sync() -> None:
    import inspect

    assert not inspect.iscoroutinefunction(register_worker)


def test_connect_consumes_otel_from_init_options(monkeypatch) -> None:
    import iii.telemetry as telemetry

    captured = {"config": None}

    def fake_init_otel(config=None, loop=None):
        captured["config"] = config

    def fake_attach_event_loop(loop):
        return None

    async def fake_do_connect(self: III) -> None:
        return None

    monkeypatch.setattr(telemetry, "init_otel", fake_init_otel)
    monkeypatch.setattr(telemetry, "attach_event_loop", fake_attach_event_loop)
    monkeypatch.setattr(III, "_do_connect", fake_do_connect)

    client = register_worker(
        "ws://fake",
        InitOptions(otel={"enabled": True, "service_name": "iii-python-init-test"}),
    )
    time.sleep(0.05)

    assert isinstance(client, III)
    assert captured["config"] is not None
    assert getattr(captured["config"], "service_name", None) == "iii-python-init-test"

    client.shutdown()
