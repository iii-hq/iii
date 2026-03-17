"""Worker metrics collection for the III Python SDK.

Collects CPU, memory (RSS, VMS, heap), and process uptime metrics.
Uses stdlib `os` and `resource` modules -- no psutil dependency required.
"""

from __future__ import annotations

import os
import time
from dataclasses import dataclass
from typing import Optional


@dataclass
class WorkerMetrics:
    """Snapshot of worker resource metrics."""

    memory_rss: int  # bytes
    memory_vms: int  # bytes
    cpu_percent: float  # 0-100
    uptime_seconds: float
    timestamp_ms: int
    memory_heap_used: int = 0  # bytes (tracemalloc if active)
    memory_heap_total: int = 0  # bytes (not directly available in Python)
    memory_external: int = 0  # bytes (Node.js concept, always 0 in Python)
    cpu_user_micros: int = 0  # microseconds
    cpu_system_micros: int = 0  # microseconds
    runtime: str = "python"


def _read_proc_status() -> tuple[int, int]:
    """Read RSS and VMS from /proc/self/status (Linux only).

    Returns (rss_bytes, vms_bytes). Falls back to (0, 0) on error.
    """
    try:
        with open("/proc/self/status") as f:
            rss = 0
            vms = 0
            for line in f:
                if line.startswith("VmRSS:"):
                    rss = int(line.split()[1]) * 1024  # kB -> bytes
                elif line.startswith("VmSize:"):
                    vms = int(line.split()[1]) * 1024
            return rss, vms
    except (OSError, ValueError, IndexError):
        return 0, 0


def _read_memory() -> tuple[int, int]:
    """Read (rss_bytes, vms_bytes) using the best available method."""
    import resource as res

    # resource.getrusage gives maxrss; on macOS it's bytes, on Linux it's kB
    usage = res.getrusage(res.RUSAGE_SELF)
    import sys

    if sys.platform == "darwin":
        rss = usage.ru_maxrss  # bytes on macOS
    else:
        rss = usage.ru_maxrss * 1024  # kB on Linux

    # Try /proc for more accurate RSS and VMS on Linux
    if sys.platform == "linux":
        proc_rss, proc_vms = _read_proc_status()
        if proc_rss > 0:
            return proc_rss, proc_vms

    return rss, 0  # VMS not available via resource module


class WorkerMetricsCollector:
    """Collects worker resource metrics with a caching pattern.

    Uses a 500ms cache to avoid redundant system calls when multiple
    gauge callbacks are invoked in quick succession.
    """

    CACHE_TTL_SECONDS: float = 0.5

    def __init__(self) -> None:
        self._start_time = time.monotonic()
        self._last_cpu_time: float = time.monotonic()
        self._last_cpu_user: float = 0.0
        self._last_cpu_system: float = 0.0
        self._cached: Optional[tuple[float, WorkerMetrics]] = None

        # Initialize CPU baseline
        try:
            cpu_times = os.times()
            self._last_cpu_user = cpu_times.user + cpu_times.children_user
            self._last_cpu_system = cpu_times.system + cpu_times.children_system
        except (AttributeError, OSError):
            pass

    def collect_cached(self) -> WorkerMetrics:
        """Return cached metrics if within TTL, otherwise collect fresh."""
        if self._cached is not None:
            ts, metrics = self._cached
            if (time.monotonic() - ts) < self.CACHE_TTL_SECONDS:
                return metrics

        metrics = self.collect()
        self._cached = (time.monotonic(), metrics)
        return metrics

    def collect(self) -> WorkerMetrics:
        """Collect a fresh snapshot of current worker metrics."""
        now = time.monotonic()

        # Memory
        rss, vms = _read_memory()

        # CPU percentage since last collection
        cpu_percent = 0.0
        cpu_user_micros = 0
        cpu_system_micros = 0
        try:
            cpu_times = os.times()
            cpu_user = cpu_times.user + cpu_times.children_user
            cpu_system = cpu_times.system + cpu_times.children_system
            elapsed = now - self._last_cpu_time

            if elapsed > 0:
                cpu_delta = (cpu_user - self._last_cpu_user) + (cpu_system - self._last_cpu_system)
                cpu_percent = min((cpu_delta / elapsed) * 100.0, 100.0)

            cpu_user_micros = int(cpu_user * 1_000_000)
            cpu_system_micros = int(cpu_system * 1_000_000)

            self._last_cpu_user = cpu_user
            self._last_cpu_system = cpu_system
            self._last_cpu_time = now
        except (AttributeError, OSError):
            pass

        # Heap metrics via tracemalloc (if active)
        memory_heap_used = 0
        try:
            import tracemalloc

            if tracemalloc.is_tracing():
                current, _ = tracemalloc.get_traced_memory()
                memory_heap_used = current
        except Exception:
            pass

        uptime = now - self._start_time

        return WorkerMetrics(
            memory_rss=rss,
            memory_vms=vms,
            cpu_percent=cpu_percent,
            uptime_seconds=uptime,
            timestamp_ms=int(time.time() * 1000),
            memory_heap_used=memory_heap_used,
            memory_heap_total=0,
            memory_external=0,
            cpu_user_micros=cpu_user_micros,
            cpu_system_micros=cpu_system_micros,
        )
