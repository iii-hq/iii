from __future__ import annotations

import json
import logging
from typing import TYPE_CHECKING, Callable, Optional

import psutil
from opentelemetry.metrics import Meter, get_meter
from opentelemetry.sdk.metrics import MeterProvider
from opentelemetry.sdk.metrics.export import (
    MetricExporter,
    MetricExportResult,
    MetricsData,
)

from .metrics import WorkerMetricsCollector, get_metrics_collector

if TYPE_CHECKING:
    from opentelemetry.sdk.metrics.view import Aggregation

log = logging.getLogger("iii.telemetry")

_system_gauges_registered = False


class WorkerGaugesOptions:
    def __init__(
        self,
        worker_id: str,
        worker_name: Optional[str] = None,
    ) -> None:
        self.worker_id = worker_id
        self.worker_name = worker_name


_registered_gauges = False
_metrics_collector: Optional[WorkerMetricsCollector] = None


def register_worker_gauges(meter: Meter, options: WorkerGaugesOptions) -> None:
    global _registered_gauges, _metrics_collector

    if _registered_gauges:
        return

    _metrics_collector = get_metrics_collector()

    base_attributes = {
        "worker.id": options.worker_id,
    }
    if options.worker_name:
        base_attributes["worker.name"] = options.worker_name

    def create_callback(metric_getter: Callable[[], Optional[float | int]]):
        def callback(options):
            collector = _metrics_collector
            if not collector:
                return

            metrics = collector.collect()
            value = metric_getter(metrics)
            if value is not None:
                attrs = dict(base_attributes)

                if metrics.k8s_identifiers:
                    if metrics.k8s_identifiers.pod_name:
                        attrs["k8s.pod.name"] = metrics.k8s_identifiers.pod_name
                    if metrics.k8s_identifiers.namespace:
                        attrs["k8s.namespace"] = metrics.k8s_identifiers.namespace
                    if metrics.k8s_identifiers.node_name:
                        attrs["k8s.node.name"] = metrics.k8s_identifiers.node_name
                    if metrics.k8s_identifiers.container_name:
                        attrs["k8s.container.name"] = metrics.k8s_identifiers.container_name

                yield options.Observation(value, attrs)
        return callback

    meter.create_observable_gauge(
        "iii.worker.memory.rss",
        callbacks=[create_callback(lambda m: m.memory_rss)],
        description="Worker resident set size in bytes",
        unit="bytes",
    )

    meter.create_observable_gauge(
        "iii.worker.memory.heap_used",
        callbacks=[create_callback(lambda m: m.memory_heap_used)],
        description="Worker heap memory used in bytes",
        unit="bytes",
    )

    meter.create_observable_gauge(
        "iii.worker.memory.heap_total",
        callbacks=[create_callback(lambda m: m.memory_heap_total)],
        description="Worker total heap memory in bytes",
        unit="bytes",
    )

    meter.create_observable_gauge(
        "iii.worker.cpu.percent",
        callbacks=[create_callback(lambda m: m.cpu_percent)],
        description="Worker CPU usage percentage",
        unit="%",
    )

    meter.create_observable_gauge(
        "iii.worker.cpu.user_seconds",
        callbacks=[create_callback(lambda m: m.cpu_user_seconds)],
        description="Worker CPU user time in seconds",
        unit="s",
    )

    meter.create_observable_gauge(
        "iii.worker.cpu.system_seconds",
        callbacks=[create_callback(lambda m: m.cpu_system_seconds)],
        description="Worker CPU system time in seconds",
        unit="s",
    )

    meter.create_observable_gauge(
        "iii.worker.uptime_seconds",
        callbacks=[create_callback(lambda m: m.uptime_seconds)],
        description="Worker uptime in seconds",
        unit="s",
    )

    meter.create_observable_gauge(
        "iii.worker.thread.count",
        callbacks=[create_callback(lambda m: m.thread_count)],
        description="Worker thread count",
        unit="count",
    )

    meter.create_observable_gauge(
        "iii.worker.fd.open",
        callbacks=[create_callback(lambda m: m.open_file_descriptors)],
        description="Open file descriptors",
        unit="count",
    )

    meter.create_observable_gauge(
        "iii.worker.connections.open",
        callbacks=[create_callback(lambda m: m.open_connections)],
        description="Open network connections",
        unit="count",
    )

    meter.create_observable_gauge(
        "iii.worker.errors.count",
        callbacks=[create_callback(lambda m: m.error_count)],
        description="Total error count",
        unit="count",
    )

    meter.create_observable_gauge(
        "iii.worker.invocations.total",
        callbacks=[create_callback(lambda m: m.invocations_total)],
        description="Total invocations",
        unit="count",
    )

    meter.create_observable_gauge(
        "iii.worker.invocations.rate",
        callbacks=[create_callback(lambda m: m.invocations_per_sec)],
        description="Invocations per second",
        unit="1/s",
    )

    def k8s_cpu_callback(options):
        collector = _metrics_collector
        if not collector:
            return
        metrics = collector.collect()
        if metrics.k8s and metrics.k8s.cpu_usage_cores is not None:
            attrs = dict(base_attributes)
            if metrics.k8s_identifiers:
                if metrics.k8s_identifiers.pod_name:
                    attrs["k8s.pod.name"] = metrics.k8s_identifiers.pod_name
                if metrics.k8s_identifiers.namespace:
                    attrs["k8s.namespace"] = metrics.k8s_identifiers.namespace
            yield options.Observation(metrics.k8s.cpu_usage_cores, attrs)

    def k8s_memory_callback(options):
        collector = _metrics_collector
        if not collector:
            return
        metrics = collector.collect()
        if metrics.k8s and metrics.k8s.memory_working_set_bytes is not None:
            attrs = dict(base_attributes)
            if metrics.k8s_identifiers:
                if metrics.k8s_identifiers.pod_name:
                    attrs["k8s.pod.name"] = metrics.k8s_identifiers.pod_name
                if metrics.k8s_identifiers.namespace:
                    attrs["k8s.namespace"] = metrics.k8s_identifiers.namespace
            yield options.Observation(metrics.k8s.memory_working_set_bytes, attrs)

    def k8s_cpu_limits_callback(options):
        collector = _metrics_collector
        if not collector:
            return
        metrics = collector.collect()
        if metrics.k8s and metrics.k8s.cpu_limits_cores is not None:
            attrs = dict(base_attributes)
            yield options.Observation(metrics.k8s.cpu_limits_cores, attrs)

    def k8s_memory_limits_callback(options):
        collector = _metrics_collector
        if not collector:
            return
        metrics = collector.collect()
        if metrics.k8s and metrics.k8s.memory_limits_bytes is not None:
            attrs = dict(base_attributes)
            yield options.Observation(metrics.k8s.memory_limits_bytes, attrs)

    meter.create_observable_gauge(
        "iii.worker.k8s.cpu.usage",
        callbacks=[k8s_cpu_callback],
        description="K8s container CPU usage in cores",
        unit="cores",
    )

    meter.create_observable_gauge(
        "iii.worker.k8s.memory.working_set",
        callbacks=[k8s_memory_callback],
        description="K8s container memory working set in bytes",
        unit="bytes",
    )

    meter.create_observable_gauge(
        "iii.worker.k8s.cpu.limits",
        callbacks=[k8s_cpu_limits_callback],
        description="K8s container CPU limits in cores",
        unit="cores",
    )

    meter.create_observable_gauge(
        "iii.worker.k8s.memory.limits",
        callbacks=[k8s_memory_limits_callback],
        description="K8s container memory limits in bytes",
        unit="bytes",
    )

    _registered_gauges = True
    log.debug("Registered worker OTEL gauges")


def stop_worker_gauges() -> None:
    global _registered_gauges, _metrics_collector
    _registered_gauges = False
    _metrics_collector = None


def register_system_gauges(meter: Meter) -> None:
    global _system_gauges_registered

    if _system_gauges_registered:
        return

    def cpu_callback(options):
        try:
            cpu_percent = psutil.cpu_percent(interval=None)
            yield options.Observation(cpu_percent / 100.0, {"system.cpu.state": "user"})
        except Exception:
            pass

    def memory_callback(options):
        try:
            mem = psutil.virtual_memory()
            yield options.Observation(mem.used, {"system.memory.state": "used"})
        except Exception:
            pass

    def network_rx_callback(options):
        try:
            net = psutil.net_io_counters()
            yield options.Observation(net.bytes_recv, {"network.io.direction": "receive"})
        except Exception:
            pass

    def network_tx_callback(options):
        try:
            net = psutil.net_io_counters()
            yield options.Observation(net.bytes_sent, {"network.io.direction": "transmit"})
        except Exception:
            pass

    def disk_read_callback(options):
        try:
            disk = psutil.disk_io_counters()
            if disk:
                yield options.Observation(disk.read_bytes, {"disk.io.direction": "read"})
        except Exception:
            pass

    def disk_write_callback(options):
        try:
            disk = psutil.disk_io_counters()
            if disk:
                yield options.Observation(disk.write_bytes, {"disk.io.direction": "write"})
        except Exception:
            pass

    meter.create_observable_gauge(
        "system.cpu.utilization",
        callbacks=[cpu_callback],
        description="System CPU utilization (0-1)",
        unit="1",
    )

    meter.create_observable_gauge(
        "system.memory.usage",
        callbacks=[memory_callback],
        description="System memory usage in bytes",
        unit="bytes",
    )

    meter.create_observable_gauge(
        "system.network.io",
        callbacks=[network_rx_callback, network_tx_callback],
        description="System network I/O in bytes",
        unit="bytes",
    )

    meter.create_observable_gauge(
        "system.disk.io",
        callbacks=[disk_read_callback, disk_write_callback],
        description="System disk I/O in bytes",
        unit="bytes",
    )

    _system_gauges_registered = True
    log.debug("Registered system OTEL gauges")


class EngineMetricsExporter(MetricExporter):
    def __init__(self, send_fn: Callable[[str, bytes], None]) -> None:
        self._send_fn = send_fn

    def export(
        self,
        metrics_data: MetricsData,
        timeout_millis: float = 10_000,
        **kwargs,
    ) -> MetricExportResult:
        try:
            data = self._serialize_metrics(metrics_data)
            if data:
                self._send_fn("M:", data)
            return MetricExportResult.SUCCESS
        except Exception as e:
            log.error(f"Failed to export metrics: {e}")
            return MetricExportResult.FAILURE

    def _serialize_metrics(self, metrics_data: MetricsData) -> Optional[bytes]:
        resource_metrics = []

        for resource in metrics_data.resource_metrics:
            scope_metrics = []
            for scope in resource.scope_metrics:
                metrics = []
                for metric in scope.metrics:
                    metric_data = {
                        "name": metric.name,
                        "description": metric.description,
                        "unit": metric.unit,
                    }

                    data_points = []
                    if hasattr(metric, "data") and hasattr(metric.data, "data_points"):
                        for dp in metric.data.data_points:
                            point = {
                                "timeUnixNano": str(dp.time_unix_nano),
                            }
                            if hasattr(dp, "value"):
                                point["asDouble"] = dp.value
                            if hasattr(dp, "attributes"):
                                point["attributes"] = [
                                    {"key": k, "value": {"stringValue": str(v)}}
                                    for k, v in dp.attributes.items()
                                ]
                            data_points.append(point)

                    if data_points:
                        metric_data["gauge"] = {"dataPoints": data_points}
                        metrics.append(metric_data)

                if metrics:
                    scope_metrics.append({
                        "scope": {
                            "name": scope.scope.name if scope.scope else "iii.worker",
                        },
                        "metrics": metrics,
                    })

            if scope_metrics:
                resource_metrics.append({
                    "resource": {
                        "attributes": [
                            {"key": k, "value": {"stringValue": str(v)}}
                            for k, v in (resource.resource.attributes if resource.resource else {}).items()
                        ],
                    },
                    "scopeMetrics": scope_metrics,
                })

        if not resource_metrics:
            return None

        return json.dumps({"resourceMetrics": resource_metrics}).encode("utf-8")

    def shutdown(self, timeout_millis: float = 30_000, **kwargs) -> None:
        pass

    def force_flush(self, timeout_millis: float = 10_000) -> bool:
        return True
