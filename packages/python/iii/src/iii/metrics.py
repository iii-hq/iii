import asyncio
import os
import time
from dataclasses import dataclass, field
from pathlib import Path
from threading import Lock
from typing import Any, Optional

import psutil


_state_lock = Lock()
_process_start_time = time.time()
_invocation_count = 0
_total_latency_ms = 0.0
_error_count = 0
_last_network_stats: dict[str, int] | None = None
_last_disk_stats: dict[str, int] | None = None
_last_cpu_time: float | None = None


def record_invocation(latency_ms: float, is_error: bool = False) -> None:
    global _invocation_count, _total_latency_ms, _error_count
    with _state_lock:
        _invocation_count += 1
        _total_latency_ms += latency_ms
        if is_error:
            _error_count += 1


def is_kubernetes() -> bool:
    return "KUBERNETES_SERVICE_HOST" in os.environ


def _read_k8s_file(filename: str) -> str | None:
    path = Path("/var/run/secrets/kubernetes.io/serviceaccount") / filename
    try:
        if path.exists():
            return path.read_text().strip()
    except Exception:
        pass
    return None


def _get_kubernetes_identifiers() -> dict[str, Any] | None:
    if not is_kubernetes():
        return None

    return {
        "cluster": os.environ.get("CLUSTER_NAME"),
        "namespace": os.environ.get("POD_NAMESPACE") or _read_k8s_file("namespace"),
        "pod_name": os.environ.get("POD_NAME") or os.environ.get("HOSTNAME"),
        "container_name": os.environ.get("CONTAINER_NAME"),
        "node_name": os.environ.get("NODE_NAME"),
        "pod_uid": os.environ.get("POD_UID") or _read_k8s_file("pod-uid"),
    }


def _read_cgroup_value(v1_path: str, v2_path: str) -> int | None:
    for p in [v2_path, v1_path]:
        try:
            path = Path(p)
            if path.exists():
                content = path.read_text().strip()
                value = int(content)
                if value != -1:
                    return value
        except (ValueError, OSError):
            continue
    return None


def _get_cgroup_cpu_usage_cores() -> float | None:
    try:
        v2_path = Path("/sys/fs/cgroup/cpu.stat")
        if v2_path.exists():
            content = v2_path.read_text()
            for line in content.splitlines():
                if line.startswith("usage_usec"):
                    usec = int(line.split()[1])
                    return usec / 1_000_000

        v1_path = Path("/sys/fs/cgroup/cpu/cpuacct.usage")
        if v1_path.exists():
            nsec = int(v1_path.read_text().strip())
            return nsec / 1_000_000_000
    except (ValueError, OSError):
        pass
    return None


def _get_cgroup_memory_working_set() -> int | None:
    return _read_cgroup_value(
        "/sys/fs/cgroup/memory/memory.usage_in_bytes",
        "/sys/fs/cgroup/memory.current",
    )


def _get_cgroup_memory_limit() -> int | None:
    value = _read_cgroup_value(
        "/sys/fs/cgroup/memory/memory.limit_in_bytes",
        "/sys/fs/cgroup/memory.max",
    )
    if value and value < 9223372036854771712:
        return value
    return None


def _get_cgroup_cpu_limit_cores() -> float | None:
    try:
        v2_path = Path("/sys/fs/cgroup/cpu.max")
        if v2_path.exists():
            content = v2_path.read_text().strip()
            if content != "max":
                parts = content.split()
                quota, period = int(parts[0]), int(parts[1])
                if quota > 0 and period > 0:
                    return quota / period

        quota_path = Path("/sys/fs/cgroup/cpu/cpu.cfs_quota_us")
        period_path = Path("/sys/fs/cgroup/cpu/cpu.cfs_period_us")
        if quota_path.exists() and period_path.exists():
            quota = int(quota_path.read_text().strip())
            period = int(period_path.read_text().strip())
            if quota > 0 and period > 0:
                return quota / period
    except (ValueError, OSError):
        pass
    return None


def _get_cgroup_cpu_throttled_seconds() -> float | None:
    try:
        v2_path = Path("/sys/fs/cgroup/cpu.stat")
        if v2_path.exists():
            content = v2_path.read_text()
            for line in content.splitlines():
                if line.startswith("throttled_usec"):
                    return int(line.split()[1]) / 1_000_000

        v1_path = Path("/sys/fs/cgroup/cpu/cpu.stat")
        if v1_path.exists():
            content = v1_path.read_text()
            for line in content.splitlines():
                if line.startswith("throttled_time"):
                    return int(line.split()[1]) / 1_000_000_000
    except (ValueError, OSError):
        pass
    return None


def _parse_k8s_memory(value: str) -> int | None:
    import re

    match = re.match(r"^(\d+(?:\.\d+)?)(Ki|Mi|Gi|Ti|Pi|Ei|K|M|G|T|P|E)?$", value)
    if not match:
        return None

    num = float(match.group(1))
    unit = match.group(2) or ""

    multipliers = {
        "": 1,
        "K": 1000,
        "M": 1000**2,
        "G": 1000**3,
        "T": 1000**4,
        "P": 1000**5,
        "E": 1000**6,
        "Ki": 1024,
        "Mi": 1024**2,
        "Gi": 1024**3,
        "Ti": 1024**4,
        "Pi": 1024**5,
        "Ei": 1024**6,
    }

    return int(num * multipliers.get(unit, 1))


def _get_kubernetes_core_metrics() -> dict[str, Any] | None:
    if not is_kubernetes():
        return None

    metrics: dict[str, Any] = {
        "uptime_seconds": int(time.time() - _process_start_time),
        "pod_phase": "Running",
        "pod_ready": True,
    }

    cpu_cores = _get_cgroup_cpu_usage_cores()
    if cpu_cores is not None:
        metrics["cpu_usage_cores"] = cpu_cores

    mem_working_set = _get_cgroup_memory_working_set()
    if mem_working_set is not None:
        metrics["memory_working_set_bytes"] = mem_working_set

    restarts = os.environ.get("CONTAINER_RESTARTS")
    if restarts:
        try:
            metrics["container_restarts_total"] = int(restarts)
        except ValueError:
            pass

    term_reason = os.environ.get("LAST_TERMINATION_REASON")
    if term_reason:
        metrics["last_termination_reason"] = term_reason

    return metrics


def _get_kubernetes_resource_metrics() -> dict[str, Any] | None:
    if not is_kubernetes():
        return None

    metrics: dict[str, Any] = {}

    cpu_limit = _get_cgroup_cpu_limit_cores()
    if cpu_limit is not None:
        metrics["cpu_limits_cores"] = cpu_limit

    mem_limit = _get_cgroup_memory_limit()
    if mem_limit is not None:
        metrics["memory_limits_bytes"] = mem_limit

    throttled = _get_cgroup_cpu_throttled_seconds()
    if throttled is not None:
        metrics["cpu_throttled_seconds_total"] = throttled

    cpu_request = os.environ.get("CPU_REQUEST")
    if cpu_request:
        if cpu_request.endswith("m"):
            metrics["cpu_requests_cores"] = int(cpu_request[:-1]) / 1000
        else:
            try:
                metrics["cpu_requests_cores"] = float(cpu_request)
            except ValueError:
                pass

    mem_request = os.environ.get("MEMORY_REQUEST")
    if mem_request:
        parsed = _parse_k8s_memory(mem_request)
        if parsed:
            metrics["memory_requests_bytes"] = parsed

    return metrics if metrics else None


def _get_filesystem_usage() -> int | None:
    try:
        disk = psutil.disk_usage("/")
        return disk.used
    except Exception:
        return None


def _get_node_pressure() -> dict[str, bool | None]:
    result: dict[str, bool | None] = {
        "memory": None,
        "disk": None,
        "pid": None,
    }

    if os.name != "posix":
        return result

    def extract_avg10(line: str) -> float | None:
        import re

        match = re.search(r"avg10=(\d+\.?\d*)", line)
        return float(match.group(1)) if match else None

    try:
        content = Path("/sys/fs/cgroup/memory.pressure").read_text()
        for line in content.splitlines():
            if line.startswith("some"):
                avg10 = extract_avg10(line)
                if avg10 is not None:
                    result["memory"] = avg10 > 10.0
    except Exception:
        pass

    try:
        content = Path("/sys/fs/cgroup/io.pressure").read_text()
        for line in content.splitlines():
            if line.startswith("some"):
                avg10 = extract_avg10(line)
                if avg10 is not None:
                    result["disk"] = avg10 > 10.0
    except Exception:
        pass

    try:
        content = Path("/sys/fs/cgroup/cpu.pressure").read_text()
        for line in content.splitlines():
            if line.startswith("some"):
                avg10 = extract_avg10(line)
                if avg10 is not None:
                    result["pid"] = avg10 > 10.0
    except Exception:
        pass

    return result


def _get_kubernetes_extended_metrics() -> dict[str, Any] | None:
    if not is_kubernetes():
        return None

    metrics: dict[str, Any] = {}

    try:
        net_io = psutil.net_io_counters()
        metrics["network_rx_bytes_total"] = net_io.bytes_recv
        metrics["network_tx_bytes_total"] = net_io.bytes_sent
    except Exception:
        pass

    fs_usage = _get_filesystem_usage()
    if fs_usage is not None:
        metrics["fs_usage_bytes"] = fs_usage

    pressure = _get_node_pressure()
    if pressure["memory"] is not None:
        metrics["node_memory_pressure"] = pressure["memory"]
    if pressure["disk"] is not None:
        metrics["node_disk_pressure"] = pressure["disk"]
    if pressure["pid"] is not None:
        metrics["node_pid_pressure"] = pressure["pid"]

    return metrics if metrics else None


def collect_metrics() -> dict[str, Any]:
    global _invocation_count, _total_latency_ms, _last_network_stats, _last_disk_stats, _last_cpu_time

    now_ms = int(time.time() * 1000)
    uptime_secs = int(time.time() - _process_start_time)

    cpu_percent = 0.0
    memory_used = 0
    memory_total = 0
    thread_count = 1
    network_rx = None
    network_tx = None
    disk_read = None
    disk_write = None
    open_fds = None

    proc = psutil.Process()

    cpu_percent = proc.cpu_percent(interval=None)

    mem_info = proc.memory_info()
    memory_used = mem_info.rss
    memory_total = psutil.virtual_memory().total

    try:
        thread_count = proc.num_threads()
    except Exception:
        thread_count = 1

    try:
        open_fds = proc.num_fds()
    except (AttributeError, psutil.Error):
        pass

    try:
        net_io = psutil.net_io_counters()
        current_net = {"rx": net_io.bytes_recv, "tx": net_io.bytes_sent}

        with _state_lock:
            if _last_network_stats:
                network_rx = current_net["rx"] - _last_network_stats["rx"]
                network_tx = current_net["tx"] - _last_network_stats["tx"]
            _last_network_stats = current_net
    except Exception:
        pass

    try:
        disk_io = psutil.disk_io_counters()
        if disk_io:
            current_disk = {"read": disk_io.read_bytes, "write": disk_io.write_bytes}

            with _state_lock:
                if _last_disk_stats:
                    disk_read = current_disk["read"] - _last_disk_stats["read"]
                    disk_write = current_disk["write"] - _last_disk_stats["write"]
                _last_disk_stats = current_disk
    except Exception:
        pass

    with _state_lock:
        inv_count = _invocation_count
        total_lat = _total_latency_ms
        err_count = _error_count
        _invocation_count = 0
        _total_latency_ms = 0.0
        _error_count = 0

    invocations_per_sec = inv_count / 5.0
    avg_latency_ms = total_lat / inv_count if inv_count > 0 else 0.0

    metrics: dict[str, Any] = {
        "collected_at_ms": now_ms,
        "process": {
            "cpu_percent": cpu_percent,
            "memory_used_bytes": memory_used,
            "memory_total_bytes": memory_total,
            "process_uptime_secs": uptime_secs,
        },
        "performance": {
            "thread_count": thread_count,
            "open_connections": 1,
            "invocations_per_sec": invocations_per_sec,
            "avg_latency_ms": avg_latency_ms,
        },
        "extended": {
            "disk_read_bytes": disk_read,
            "disk_write_bytes": disk_write,
            "network_rx_bytes": network_rx,
            "network_tx_bytes": network_tx,
            "open_file_descriptors": open_fds,
            "error_count": err_count,
        },
    }

    if is_kubernetes():
        metrics["k8s_identifiers"] = _get_kubernetes_identifiers()
        metrics["k8s_core"] = _get_kubernetes_core_metrics()
        metrics["k8s_resources"] = _get_kubernetes_resource_metrics()
        metrics["k8s_extended"] = _get_kubernetes_extended_metrics()

    return metrics


@dataclass
class K8sIdentifiersData:
    cluster: Optional[str] = None
    namespace: Optional[str] = None
    pod_name: Optional[str] = None
    container_name: Optional[str] = None
    node_name: Optional[str] = None
    pod_uid: Optional[str] = None


@dataclass
class K8sMetricsData:
    cpu_usage_cores: Optional[float] = None
    memory_working_set_bytes: Optional[int] = None
    cpu_limits_cores: Optional[float] = None
    memory_limits_bytes: Optional[int] = None
    cpu_throttled_seconds_total: Optional[float] = None
    cpu_requests_cores: Optional[float] = None
    memory_requests_bytes: Optional[int] = None


@dataclass
class WorkerMetricsSnapshot:
    memory_rss: Optional[int] = None
    memory_heap_used: Optional[int] = None
    memory_heap_total: Optional[int] = None
    cpu_percent: Optional[float] = None
    cpu_user_seconds: Optional[float] = None
    cpu_system_seconds: Optional[float] = None
    uptime_seconds: Optional[int] = None
    thread_count: Optional[int] = None
    open_file_descriptors: Optional[int] = None
    open_connections: Optional[int] = None
    error_count: Optional[int] = None
    invocations_total: Optional[int] = None
    invocations_per_sec: Optional[float] = None
    k8s_identifiers: Optional[K8sIdentifiersData] = None
    k8s: Optional[K8sMetricsData] = None


class WorkerMetricsCollector:
    def __init__(self) -> None:
        self._proc = psutil.Process()
        self._proc.cpu_percent(interval=None)
        self._invocations_total = 0

    def collect(self) -> WorkerMetricsSnapshot:
        raw = collect_metrics()

        process = raw.get("process", {})
        perf = raw.get("performance", {})
        ext = raw.get("extended", {})

        cpu_times = self._proc.cpu_times()

        k8s_ids = None
        raw_ids = raw.get("k8s_identifiers")
        if raw_ids:
            k8s_ids = K8sIdentifiersData(
                cluster=raw_ids.get("cluster"),
                namespace=raw_ids.get("namespace"),
                pod_name=raw_ids.get("pod_name"),
                container_name=raw_ids.get("container_name"),
                node_name=raw_ids.get("node_name"),
                pod_uid=raw_ids.get("pod_uid"),
            )

        k8s_metrics = None
        raw_core = raw.get("k8s_core")
        raw_res = raw.get("k8s_resources")
        if raw_core or raw_res:
            k8s_metrics = K8sMetricsData(
                cpu_usage_cores=(raw_core or {}).get("cpu_usage_cores"),
                memory_working_set_bytes=(raw_core or {}).get("memory_working_set_bytes"),
                cpu_limits_cores=(raw_res or {}).get("cpu_limits_cores"),
                memory_limits_bytes=(raw_res or {}).get("memory_limits_bytes"),
                cpu_throttled_seconds_total=(raw_res or {}).get("cpu_throttled_seconds_total"),
                cpu_requests_cores=(raw_res or {}).get("cpu_requests_cores"),
                memory_requests_bytes=(raw_res or {}).get("memory_requests_bytes"),
            )

        inv_total_delta = int(perf.get("invocations_per_sec", 0) * 5)
        self._invocations_total += inv_total_delta

        return WorkerMetricsSnapshot(
            memory_rss=process.get("memory_used_bytes"),
            memory_heap_used=process.get("memory_used_bytes"),
            memory_heap_total=process.get("memory_total_bytes"),
            cpu_percent=process.get("cpu_percent"),
            cpu_user_seconds=cpu_times.user,
            cpu_system_seconds=cpu_times.system,
            uptime_seconds=process.get("process_uptime_secs"),
            thread_count=perf.get("thread_count"),
            open_file_descriptors=ext.get("open_file_descriptors"),
            open_connections=perf.get("open_connections"),
            error_count=ext.get("error_count"),
            invocations_total=self._invocations_total,
            invocations_per_sec=perf.get("invocations_per_sec"),
            k8s_identifiers=k8s_ids,
            k8s=k8s_metrics,
        )


_collector_instance: Optional[WorkerMetricsCollector] = None
_collector_lock = Lock()


def get_metrics_collector() -> WorkerMetricsCollector:
    global _collector_instance
    with _collector_lock:
        if _collector_instance is None:
            _collector_instance = WorkerMetricsCollector()
        return _collector_instance


class MetricsReporter:
    def __init__(
        self,
        report_fn,
        interval_seconds: float = 5.0,
    ):
        self._report_fn = report_fn
        self._interval = interval_seconds
        self._running = False
        self._task = None

    async def start(self):
        if self._running:
            return

        self._running = True

        psutil.Process().cpu_percent(interval=None)

        async def _loop():
            while self._running:
                await asyncio.sleep(self._interval)
                if self._running:
                    metrics = collect_metrics()
                    self._report_fn(metrics)

        self._task = asyncio.create_task(_loop())

    async def stop(self):
        self._running = False
        if self._task:
            self._task.cancel()
            try:
                await self._task
            except asyncio.CancelledError:
                pass
            self._task = None
