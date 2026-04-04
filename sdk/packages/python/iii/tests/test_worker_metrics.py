from __future__ import annotations

import io
import sys
import types
from unittest.mock import Mock

import pytest

import iii.otel_worker_gauges as gauges_module
import iii.worker_metrics as metrics_module


@pytest.fixture(autouse=True)
def reset_worker_gauges() -> None:
    gauges_module.stop_worker_gauges()
    yield
    gauges_module.stop_worker_gauges()


def test_read_proc_status_parses_linux_memory(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(
        "builtins.open",
        lambda *_args, **_kwargs: io.StringIO("VmRSS:\t12 kB\nVmSize:\t34 kB\n"),
    )

    rss, vms = metrics_module._read_proc_status()

    assert rss == 12 * 1024
    assert vms == 34 * 1024


def test_read_proc_status_returns_zero_on_error(monkeypatch: pytest.MonkeyPatch) -> None:
    def raise_os_error(*_args: object, **_kwargs: object) -> io.StringIO:
        raise OSError("boom")

    monkeypatch.setattr("builtins.open", raise_os_error)

    assert metrics_module._read_proc_status() == (0, 0)


def test_read_memory_uses_proc_status_on_linux(monkeypatch: pytest.MonkeyPatch) -> None:
    resource_module = types.SimpleNamespace(
        RUSAGE_SELF=1,
        getrusage=lambda _self: types.SimpleNamespace(ru_maxrss=5),
    )
    monkeypatch.setitem(sys.modules, "resource", resource_module)
    monkeypatch.setattr(sys, "platform", "linux")
    monkeypatch.setattr(metrics_module, "_read_proc_status", lambda: (2048, 4096))

    assert metrics_module._read_memory() == (2048, 4096)


def test_read_memory_uses_maxrss_on_darwin(monkeypatch: pytest.MonkeyPatch) -> None:
    resource_module = types.SimpleNamespace(
        RUSAGE_SELF=1,
        getrusage=lambda _self: types.SimpleNamespace(ru_maxrss=321),
    )
    monkeypatch.setitem(sys.modules, "resource", resource_module)
    monkeypatch.setattr(sys, "platform", "darwin")

    assert metrics_module._read_memory() == (321, 0)


def test_worker_metrics_collector_computes_cpu_and_uses_cache(monkeypatch: pytest.MonkeyPatch) -> None:
    monotonic_values = iter([10.0, 10.0, 10.5, 10.75, 10.8, 10.9])
    timestamp_values = iter([1_700_000_000.0, 1_700_000_000.5])
    os_times_values = iter(
        [
            types.SimpleNamespace(user=1.0, system=0.5, children_user=0.0, children_system=0.0),
            types.SimpleNamespace(user=1.3, system=0.7, children_user=0.0, children_system=0.0),
            types.SimpleNamespace(user=1.31, system=0.71, children_user=0.0, children_system=0.0),
        ]
    )
    collect_calls = Mock(return_value=(8192, 16384))

    monkeypatch.setattr(metrics_module.time, "monotonic", lambda: next(monotonic_values))
    monkeypatch.setattr(metrics_module.time, "time", lambda: next(timestamp_values))
    monkeypatch.setattr(metrics_module.os, "times", lambda: next(os_times_values))
    monkeypatch.setattr(metrics_module, "_read_memory", collect_calls)

    collector = metrics_module.WorkerMetricsCollector()
    metrics = collector.collect()
    cached = collector.collect_cached()
    cached_again = collector.collect_cached()

    assert metrics.memory_rss == 8192
    assert metrics.memory_vms == 16384
    assert metrics.cpu_percent == pytest.approx(100.0)
    assert metrics.uptime_seconds == pytest.approx(0.5)
    assert metrics.timestamp_ms == 1_700_000_000_000
    assert cached.timestamp_ms == 1_700_000_000_500
    assert cached is cached_again
    assert collect_calls.call_count == 2


def test_register_worker_gauges_observes_metrics_once() -> None:
    meter = Mock()
    gauges = [object() for _ in range(9)]
    meter.create_observable_gauge.side_effect = gauges
    batch_callback: list[object] = []
    meter.add_batch_observable_callback.side_effect = lambda callback, _items: batch_callback.append(callback)

    fake_collector = Mock()
    fake_collector.collect_cached.return_value = metrics_module.WorkerMetrics(
        memory_rss=10,
        memory_vms=20,
        cpu_percent=30.5,
        uptime_seconds=40.0,
        timestamp_ms=50,
    )
    original_collector = gauges_module.WorkerMetricsCollector
    gauges_module.WorkerMetricsCollector = Mock(return_value=fake_collector)

    try:
        gauges_module.register_worker_gauges(meter, "worker-1", "py-worker")
        gauges_module.register_worker_gauges(meter, "worker-ignored")

        result = Mock()
        callback = batch_callback[0]
        callback(result)

        assert meter.create_observable_gauge.call_count == 9
        assert result.observe.call_count == 9
        result.observe.assert_any_call(
            gauges[0],
            30.5,
            {"worker.id": "worker-1", "worker.name": "py-worker"},
        )
    finally:
        gauges_module.WorkerMetricsCollector = original_collector


def test_stop_worker_gauges_resets_registration_state() -> None:
    meter = Mock()
    meter.create_observable_gauge.side_effect = lambda **_kwargs: object()
    meter.add_batch_observable_callback.side_effect = lambda *_args: None

    gauges_module.register_worker_gauges(meter, "worker-2")
    gauges_module.stop_worker_gauges()
    gauges_module.register_worker_gauges(meter, "worker-3")

    assert meter.create_observable_gauge.call_count == 18
