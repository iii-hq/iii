use std::sync::Mutex;
use std::time::Instant;
use sysinfo::{Pid, ProcessesToUpdate, System};

/// Collected worker metrics snapshot
#[derive(Debug, Clone)]
pub struct WorkerMetrics {
    pub memory_rss: u64,
    pub memory_virtual: u64,
    pub cpu_percent: f32,
    pub uptime_seconds: f64,
    pub timestamp_ms: u64,
    pub runtime: &'static str,
}

/// Collects system metrics for the current process
pub struct WorkerMetricsCollector {
    system: Mutex<System>,
    pid: Pid,
    start_time: Instant,
    cached: Mutex<Option<(Instant, WorkerMetrics)>>,
}

impl Default for WorkerMetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkerMetricsCollector {
    pub fn new() -> Self {
        let pid = Pid::from_u32(std::process::id());
        let system = System::new();
        Self {
            system: Mutex::new(system),
            pid,
            start_time: Instant::now(),
            cached: Mutex::new(None),
        }
    }

    /// Collect a snapshot using a 500ms cache to avoid redundant sysinfo refreshes.
    ///
    /// Multiple gauge callbacks are invoked in quick succession during each
    /// metrics collection cycle. This method ensures `sysinfo` is refreshed
    /// at most once per 500ms window.
    pub fn collect_cached(&self) -> WorkerMetrics {
        const CACHE_TTL: std::time::Duration = std::time::Duration::from_millis(500);

        let mut cached = self.cached.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((ts, ref metrics)) = *cached
            && ts.elapsed() < CACHE_TTL
        {
            return metrics.clone();
        }

        let metrics = self.collect();
        *cached = Some((Instant::now(), metrics.clone()));
        metrics
    }

    /// Collect a snapshot of current metrics
    pub fn collect(&self) -> WorkerMetrics {
        let mut system = self.system.lock().unwrap_or_else(|e| e.into_inner());
        system.refresh_processes(ProcessesToUpdate::Some(&[self.pid]), false);

        let (memory_rss, memory_virtual, cpu_percent) = system
            .process(self.pid)
            .map(|p| (p.memory(), p.virtual_memory(), p.cpu_usage()))
            .unwrap_or((0, 0, 0.0));

        let uptime = self.start_time.elapsed().as_secs_f64();
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        WorkerMetrics {
            memory_rss,
            memory_virtual,
            cpu_percent,
            uptime_seconds: uptime,
            timestamp_ms,
            runtime: "rust",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_metrics() {
        let collector = WorkerMetricsCollector::new();
        let metrics = collector.collect();

        assert_eq!(metrics.runtime, "rust");
        assert!(metrics.timestamp_ms > 0);
        assert!(metrics.uptime_seconds >= 0.0);
    }

    #[test]
    fn test_collect_multiple_snapshots() {
        let collector = WorkerMetricsCollector::new();
        let m1 = collector.collect();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let m2 = collector.collect();

        assert!(m2.uptime_seconds >= m1.uptime_seconds);
        assert!(m2.timestamp_ms >= m1.timestamp_ms);
    }
}
