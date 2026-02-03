"""Real system metrics collection for iii-sdk.

Collects CPU, memory, network, disk metrics and K8s info when available.
Uses the `psutil` library for cross-platform support.
"""

import asyncio
import os
import time
from pathlib import Path
from threading import Lock
from typing import Any

import psutil


# Global state for tracking metrics over time
_state_lock = Lock()
_process_start_time = time.time()
_invocation_count = 0
_total_latency_ms = 0.0
_error_count = 0
_last_network_stats: dict[str, int] | None = None
_last_disk_stats: dict[str, int] | None = None
_last_cpu_time: float | None = None


def record_invocation(latency_ms: float, is_error: bool = False) -> None:
    """Record an invocation for throughput/latency calculations."""
    global _invocation_count, _total_latency_ms, _error_count
    with _state_lock:
        _invocation_count += 1
        _total_latency_ms += latency_ms
        if is_error:
            _error_count += 1


def is_kubernetes() -> bool:
    """Check if running inside Kubernetes."""
    return "KUBERNETES_SERVICE_HOST" in os.environ


def _read_k8s_file(filename: str) -> str | None:
    """Read Kubernetes downward API files."""
    path = Path("/var/run/secrets/kubernetes.io/serviceaccount") / filename
    try:
        if path.exists():
            return path.read_text().strip()
    except Exception:
        pass
    return None


def _get_kubernetes_identifiers() -> dict[str, Any] | None:
    """Get Kubernetes identifiers from environment/downward API."""
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
    """Read a cgroup file and return its numeric value."""
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
    """Get CPU usage in cores from cgroup."""
    try:
        # cgroup v2
        v2_path = Path("/sys/fs/cgroup/cpu.stat")
        if v2_path.exists():
            content = v2_path.read_text()
            for line in content.splitlines():
                if line.startswith("usage_usec"):
                    usec = int(line.split()[1])
                    return usec / 1_000_000

        # cgroup v1
        v1_path = Path("/sys/fs/cgroup/cpu/cpuacct.usage")
        if v1_path.exists():
            nsec = int(v1_path.read_text().strip())
            return nsec / 1_000_000_000
    except (ValueError, OSError):
        pass
    return None


def _get_cgroup_memory_working_set() -> int | None:
    """Get memory working set bytes from cgroup."""
    return _read_cgroup_value(
        "/sys/fs/cgroup/memory/memory.usage_in_bytes",
        "/sys/fs/cgroup/memory.current",
    )


def _get_cgroup_memory_limit() -> int | None:
    """Get memory limit from cgroup."""
    value = _read_cgroup_value(
        "/sys/fs/cgroup/memory/memory.limit_in_bytes",
        "/sys/fs/cgroup/memory.max",
    )
    # Very large values mean no limit
    if value and value < 9223372036854771712:
        return value
    return None


def _get_cgroup_cpu_limit_cores() -> float | None:
    """Get CPU limit in cores from cgroup."""
    try:
        # cgroup v2
        v2_path = Path("/sys/fs/cgroup/cpu.max")
        if v2_path.exists():
            content = v2_path.read_text().strip()
            if content != "max":
                parts = content.split()
                quota, period = int(parts[0]), int(parts[1])
                if quota > 0 and period > 0:
                    return quota / period

        # cgroup v1
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
    """Get CPU throttled time from cgroup."""
    try:
        # cgroup v2
        v2_path = Path("/sys/fs/cgroup/cpu.stat")
        if v2_path.exists():
            content = v2_path.read_text()
            for line in content.splitlines():
                if line.startswith("throttled_usec"):
                    return int(line.split()[1]) / 1_000_000

        # cgroup v1
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
    """Parse Kubernetes memory strings (e.g., '256Mi', '1Gi')."""
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
    """Get Kubernetes core metrics."""
    if not is_kubernetes():
        return None

    metrics: dict[str, Any] = {
        "uptime_seconds": int(time.time() - _process_start_time),
        "pod_phase": "Running",
        "pod_ready": True,
    }

    # Add cgroup-based metrics
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
    """Get Kubernetes resource metrics (limits, requests, throttling)."""
    if not is_kubernetes():
        return None

    metrics: dict[str, Any] = {}

    # CPU/memory limits from cgroup
    cpu_limit = _get_cgroup_cpu_limit_cores()
    if cpu_limit is not None:
        metrics["cpu_limits_cores"] = cpu_limit

    mem_limit = _get_cgroup_memory_limit()
    if mem_limit is not None:
        metrics["memory_limits_bytes"] = mem_limit

    # CPU throttling
    throttled = _get_cgroup_cpu_throttled_seconds()
    if throttled is not None:
        metrics["cpu_throttled_seconds_total"] = throttled

    # Requests from downward API env vars
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
    """Get filesystem usage bytes for root filesystem."""
    try:
        disk = psutil.disk_usage("/")
        return disk.used
    except Exception:
        return None


def _get_node_pressure() -> dict[str, bool | None]:
    """Get node pressure conditions from cgroup pressure files."""
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

    # Memory pressure
    try:
        content = Path("/sys/fs/cgroup/memory.pressure").read_text()
        for line in content.splitlines():
            if line.startswith("some"):
                avg10 = extract_avg10(line)
                if avg10 is not None:
                    result["memory"] = avg10 > 10.0
    except Exception:
        pass

    # IO/Disk pressure
    try:
        content = Path("/sys/fs/cgroup/io.pressure").read_text()
        for line in content.splitlines():
            if line.startswith("some"):
                avg10 = extract_avg10(line)
                if avg10 is not None:
                    result["disk"] = avg10 > 10.0
    except Exception:
        pass

    # CPU/PID pressure
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
    """Get Kubernetes extended metrics."""
    if not is_kubernetes():
        return None

    metrics: dict[str, Any] = {}

    # Network stats
    try:
        net_io = psutil.net_io_counters()
        metrics["network_rx_bytes_total"] = net_io.bytes_recv
        metrics["network_tx_bytes_total"] = net_io.bytes_sent
    except Exception:
        pass

    # Filesystem usage
    fs_usage = _get_filesystem_usage()
    if fs_usage is not None:
        metrics["fs_usage_bytes"] = fs_usage

    # Node pressure conditions from cgroup v2 pressure files
    pressure = _get_node_pressure()
    if pressure["memory"] is not None:
        metrics["node_memory_pressure"] = pressure["memory"]
    if pressure["disk"] is not None:
        metrics["node_disk_pressure"] = pressure["disk"]
    if pressure["pid"] is not None:
        metrics["node_pid_pressure"] = pressure["pid"]

    return metrics if metrics else None


def collect_metrics() -> dict[str, Any]:
    """Collect all system metrics.

    Returns a WorkerMetrics-compatible dictionary.
    """
    global _invocation_count, _total_latency_ms, _last_network_stats, _last_disk_stats, _last_cpu_time

    now_ms = int(time.time() * 1000)
    uptime_secs = int(time.time() - _process_start_time)

    # Default values if psutil not available
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

    # CPU percent (since last call)
    cpu_percent = proc.cpu_percent(interval=None)

    # Memory
    mem_info = proc.memory_info()
    memory_used = mem_info.rss
    memory_total = psutil.virtual_memory().total

    # Thread count
    try:
        thread_count = proc.num_threads()
    except Exception:
        thread_count = 1

    # Open file descriptors (Unix only)
    try:
        open_fds = proc.num_fds()
    except (AttributeError, psutil.Error):
        pass

    # Network I/O (delta)
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

    # Disk I/O (delta)
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

    # Calculate invocation metrics and reset counters
    with _state_lock:
        inv_count = _invocation_count
        total_lat = _total_latency_ms
        err_count = _error_count
        _invocation_count = 0
        _total_latency_ms = 0.0

    # Assuming 5s reporting interval
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
            "open_connections": 1,  # WebSocket connection
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

    # Add K8s metrics if running in Kubernetes
    if is_kubernetes():
        metrics["k8s_identifiers"] = _get_kubernetes_identifiers()
        metrics["k8s_core"] = _get_kubernetes_core_metrics()
        metrics["k8s_resources"] = _get_kubernetes_resource_metrics()
        metrics["k8s_extended"] = _get_kubernetes_extended_metrics()

    return metrics


class MetricsReporter:
    """Auto-reporter that periodically collects and sends metrics."""

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
        """Start the metrics reporter."""
        if self._running:
            return

        self._running = True

        # Initial warmup call to establish baselines
        psutil.Process().cpu_percent(interval=None)

        async def _loop():
            while self._running:
                await asyncio.sleep(self._interval)
                if self._running:
                    metrics = collect_metrics()
                    self._report_fn(metrics)

        self._task = asyncio.create_task(_loop())

    async def stop(self):
        """Stop the metrics reporter."""
        self._running = False
        if self._task:
            self._task.cancel()
            try:
                await self._task
            except asyncio.CancelledError:
                pass
            self._task = None
