"""Observable OTel gauges for worker metrics in the III Python SDK.

Registers observable gauges with the OTel meter for:
  - iii.worker.cpu.percent
  - iii.worker.cpu.user_micros
  - iii.worker.cpu.system_micros
  - iii.worker.memory.rss
  - iii.worker.memory.vms
  - iii.worker.memory.heap_used
  - iii.worker.memory.heap_total
  - iii.worker.memory.external
  - iii.worker.uptime_seconds

Uses a batch observable callback for efficient single-collection-per-cycle.
"""

from __future__ import annotations

from typing import Any, Optional

from .worker_metrics import WorkerMetricsCollector

_registered: bool = False
_collector: Optional[WorkerMetricsCollector] = None
_meter: Any = None
_batch_callback_ref: Any = None
_observables: list[Any] | None = None


def register_worker_gauges(
    meter: Any,
    worker_id: str,
    worker_name: Optional[str] = None,
) -> None:
    """Register observable gauges for worker metrics with the given meter.

    Args:
        meter: An opentelemetry.metrics.Meter instance.
        worker_id: Unique identifier for the worker.
        worker_name: Optional human-readable worker name.
    """
    global _registered, _collector, _meter, _batch_callback_ref, _observables

    if _registered:
        return

    _collector = WorkerMetricsCollector()

    base_attributes: dict[str, str] = {"worker.id": worker_id}
    if worker_name:
        base_attributes["worker.name"] = worker_name

    cpu_percent = meter.create_observable_gauge(
        name="iii.worker.cpu.percent",
        description="Worker CPU usage percentage",
        unit="%",
    )

    memory_rss = meter.create_observable_gauge(
        name="iii.worker.memory.rss",
        description="Worker resident set size in bytes",
        unit="bytes",
    )

    memory_vms = meter.create_observable_gauge(
        name="iii.worker.memory.vms",
        description="Worker virtual memory in bytes",
        unit="bytes",
    )

    uptime_seconds = meter.create_observable_gauge(
        name="iii.worker.uptime_seconds",
        description="Worker uptime in seconds",
        unit="s",
    )

    memory_heap_used = meter.create_observable_gauge(
        name="iii.worker.memory.heap_used",
        description="Worker heap memory used in bytes",
        unit="bytes",
    )

    memory_heap_total = meter.create_observable_gauge(
        name="iii.worker.memory.heap_total",
        description="Worker total heap memory in bytes",
        unit="bytes",
    )

    memory_external = meter.create_observable_gauge(
        name="iii.worker.memory.external",
        description="Worker external memory in bytes",
        unit="bytes",
    )

    cpu_user_micros = meter.create_observable_gauge(
        name="iii.worker.cpu.user_micros",
        description="Worker CPU user time in microseconds",
        unit="us",
    )

    cpu_system_micros = meter.create_observable_gauge(
        name="iii.worker.cpu.system_micros",
        description="Worker CPU system time in microseconds",
        unit="us",
    )

    def _batch_callback(batch_result: Any) -> None:
        if _collector is None:
            return

        metrics = _collector.collect_cached()

        batch_result.observe(cpu_percent, metrics.cpu_percent, base_attributes)
        batch_result.observe(memory_rss, metrics.memory_rss, base_attributes)
        batch_result.observe(memory_vms, metrics.memory_vms, base_attributes)
        batch_result.observe(uptime_seconds, metrics.uptime_seconds, base_attributes)
        batch_result.observe(memory_heap_used, metrics.memory_heap_used, base_attributes)
        batch_result.observe(memory_heap_total, metrics.memory_heap_total, base_attributes)
        batch_result.observe(memory_external, metrics.memory_external, base_attributes)
        batch_result.observe(cpu_user_micros, metrics.cpu_user_micros, base_attributes)
        batch_result.observe(cpu_system_micros, metrics.cpu_system_micros, base_attributes)

    observables = [
        cpu_percent,
        memory_rss,
        memory_vms,
        uptime_seconds,
        memory_heap_used,
        memory_heap_total,
        memory_external,
        cpu_user_micros,
        cpu_system_micros,
    ]
    meter.add_batch_observable_callback(_batch_callback, observables)

    _meter = meter
    _batch_callback_ref = _batch_callback
    _observables = observables

    _registered = True


def stop_worker_gauges() -> None:
    """Stop worker gauge collection and release resources."""
    global _registered, _collector, _meter, _batch_callback_ref, _observables
    if _meter is not None and _batch_callback_ref is not None and _observables is not None:
        try:
            _meter.remove_batch_observable_callback(_batch_callback_ref, _observables)
        except Exception:
            pass
    _collector = None
    _meter = None
    _batch_callback_ref = None
    _observables = None
    _registered = False
