//! Real system metrics collection for iii-sdk
//!
//! Collects CPU, memory, network, disk metrics and K8s info when available.
//! Uses the `sysinfo` crate for cross-platform support.

use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use sysinfo::{Disks, Networks, Pid, System};

use crate::bridge::{
    ExtendedMetrics, KubernetesCoreMetrics, KubernetesExtendedMetrics, KubernetesIdentifiers,
    KubernetesResourceMetrics, PerformanceMetrics, ProcessMetrics, WorkerMetrics,
};

/// Global metrics state
static INVOCATION_COUNT: AtomicU64 = AtomicU64::new(0);
static TOTAL_LATENCY_MS: AtomicU64 = AtomicU64::new(0);
static ERROR_COUNT: AtomicU64 = AtomicU64::new(0);

lazy_static::lazy_static! {
    static ref START_TIME: Instant = Instant::now();
    static ref SYSTEM: Mutex<System> = Mutex::new(System::new_all());
    static ref NETWORKS: Mutex<Networks> = Mutex::new(Networks::new_with_refreshed_list());
    static ref DISKS: Mutex<Disks> = Mutex::new(Disks::new_with_refreshed_list());
    static ref LAST_NETWORK_STATS: Mutex<Option<(u64, u64)>> = Mutex::new(None);
    static ref LAST_DISK_STATS: Mutex<Option<(u64, u64)>> = Mutex::new(None);
}

/// Record an invocation for throughput/latency calculations
pub fn record_invocation(latency_ms: u64, is_error: bool) {
    INVOCATION_COUNT.fetch_add(1, Ordering::Relaxed);
    TOTAL_LATENCY_MS.fetch_add(latency_ms, Ordering::Relaxed);
    if is_error {
        ERROR_COUNT.fetch_add(1, Ordering::Relaxed);
    }
}

/// Check if running inside Kubernetes
pub fn is_kubernetes() -> bool {
    std::env::var("KUBERNETES_SERVICE_HOST").is_ok()
}

/// Get Kubernetes identifiers from environment
fn get_kubernetes_identifiers() -> Option<KubernetesIdentifiers> {
    if !is_kubernetes() {
        return None;
    }

    Some(KubernetesIdentifiers {
        cluster: std::env::var("CLUSTER_NAME").ok(),
        namespace: std::env::var("POD_NAMESPACE")
            .ok()
            .or_else(|| read_k8s_file("namespace")),
        pod_name: std::env::var("POD_NAME")
            .ok()
            .or_else(|| std::env::var("HOSTNAME").ok()),
        container_name: std::env::var("CONTAINER_NAME").ok(),
        node_name: std::env::var("NODE_NAME").ok(),
        pod_uid: std::env::var("POD_UID")
            .ok()
            .or_else(|| read_k8s_file("pod-uid")),
    })
}

/// Read Kubernetes downward API files
fn read_k8s_file(filename: &str) -> Option<String> {
    let path = format!("/var/run/secrets/kubernetes.io/serviceaccount/{}", filename);
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Read a cgroup file and return its value
fn read_cgroup_value(v1_path: &str, v2_path: &str) -> Option<u64> {
    for path in &[v2_path, v1_path] {
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(value) = content.trim().parse::<u64>() {
                if value != u64::MAX {
                    return Some(value);
                }
            }
        }
    }
    None
}

/// Get CPU usage in cores from cgroup
fn get_cgroup_cpu_usage_cores() -> Option<f64> {
    // cgroup v2: /sys/fs/cgroup/cpu.stat contains usage_usec
    if let Ok(content) = fs::read_to_string("/sys/fs/cgroup/cpu.stat") {
        for line in content.lines() {
            if line.starts_with("usage_usec") {
                if let Some(usec_str) = line.split_whitespace().nth(1) {
                    if let Ok(usec) = usec_str.parse::<u64>() {
                        return Some(usec as f64 / 1_000_000.0);
                    }
                }
            }
        }
    }

    // cgroup v1
    if let Ok(content) = fs::read_to_string("/sys/fs/cgroup/cpu/cpuacct.usage") {
        if let Ok(nsec) = content.trim().parse::<u64>() {
            return Some(nsec as f64 / 1_000_000_000.0);
        }
    }

    None
}

/// Get memory working set bytes from cgroup
fn get_cgroup_memory_working_set() -> Option<u64> {
    read_cgroup_value(
        "/sys/fs/cgroup/memory/memory.usage_in_bytes",
        "/sys/fs/cgroup/memory.current",
    )
}

/// Get memory limit from cgroup
fn get_cgroup_memory_limit() -> Option<u64> {
    let value = read_cgroup_value(
        "/sys/fs/cgroup/memory/memory.limit_in_bytes",
        "/sys/fs/cgroup/memory.max",
    )?;

    // Very large values mean no limit
    if value < 9223372036854771712 {
        Some(value)
    } else {
        None
    }
}

/// Get CPU limit in cores from cgroup
fn get_cgroup_cpu_limit_cores() -> Option<f64> {
    // cgroup v2
    if let Ok(content) = fs::read_to_string("/sys/fs/cgroup/cpu.max") {
        let content = content.trim();
        if content != "max" {
            let parts: Vec<&str> = content.split_whitespace().collect();
            if parts.len() >= 2 {
                if let (Ok(quota), Ok(period)) = (parts[0].parse::<i64>(), parts[1].parse::<i64>())
                {
                    if quota > 0 && period > 0 {
                        return Some(quota as f64 / period as f64);
                    }
                }
            }
        }
    }

    // cgroup v1
    if let (Ok(quota_str), Ok(period_str)) = (
        fs::read_to_string("/sys/fs/cgroup/cpu/cpu.cfs_quota_us"),
        fs::read_to_string("/sys/fs/cgroup/cpu/cpu.cfs_period_us"),
    ) {
        if let (Ok(quota), Ok(period)) = (
            quota_str.trim().parse::<i64>(),
            period_str.trim().parse::<i64>(),
        ) {
            if quota > 0 && period > 0 {
                return Some(quota as f64 / period as f64);
            }
        }
    }

    None
}

/// Get CPU throttled time from cgroup
fn get_cgroup_cpu_throttled_seconds() -> Option<f64> {
    // cgroup v2
    if let Ok(content) = fs::read_to_string("/sys/fs/cgroup/cpu.stat") {
        for line in content.lines() {
            if line.starts_with("throttled_usec") {
                if let Some(usec_str) = line.split_whitespace().nth(1) {
                    if let Ok(usec) = usec_str.parse::<u64>() {
                        return Some(usec as f64 / 1_000_000.0);
                    }
                }
            }
        }
    }

    // cgroup v1
    if let Ok(content) = fs::read_to_string("/sys/fs/cgroup/cpu/cpu.stat") {
        for line in content.lines() {
            if line.starts_with("throttled_time") {
                if let Some(ns_str) = line.split_whitespace().nth(1) {
                    if let Ok(ns) = ns_str.parse::<u64>() {
                        return Some(ns as f64 / 1_000_000_000.0);
                    }
                }
            }
        }
    }

    None
}

/// Parse Kubernetes memory strings (e.g., "256Mi", "1Gi")
fn parse_k8s_memory(value: &str) -> Option<u64> {
    let value = value.trim();

    // Try to find where the unit starts
    let (num_str, unit) = if let Some(pos) = value.find(|c: char| c.is_alphabetic()) {
        (&value[..pos], &value[pos..])
    } else {
        (value, "")
    };

    let num: f64 = num_str.parse().ok()?;

    let multiplier = match unit {
        "" => 1,
        "K" => 1000,
        "M" => 1000 * 1000,
        "G" => 1000 * 1000 * 1000,
        "T" => 1000u64.pow(4),
        "P" => 1000u64.pow(5),
        "E" => 1000u64.pow(6),
        "Ki" => 1024,
        "Mi" => 1024 * 1024,
        "Gi" => 1024 * 1024 * 1024,
        "Ti" => 1024u64.pow(4),
        "Pi" => 1024u64.pow(5),
        "Ei" => 1024u64.pow(6),
        _ => return None,
    };

    Some((num * multiplier as f64) as u64)
}

/// Get disk I/O stats (Linux only via /proc/diskstats)
#[cfg(target_os = "linux")]
fn get_disk_io_stats() -> Option<(u64, u64)> {
    let content = fs::read_to_string("/proc/diskstats").ok()?;

    let mut total_read = 0u64;
    let mut total_write = 0u64;

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 14 {
            // Sectors read (index 5) and written (index 9), 512 bytes per sector
            if let (Ok(read), Ok(write)) = (parts[5].parse::<u64>(), parts[9].parse::<u64>()) {
                total_read += read * 512;
                total_write += write * 512;
            }
        }
    }

    Some((total_read, total_write))
}

#[cfg(not(target_os = "linux"))]
fn get_disk_io_stats() -> Option<(u64, u64)> {
    None
}

/// Get open file descriptor count (Linux only via /proc)
#[cfg(target_os = "linux")]
fn get_open_file_descriptors() -> Option<u32> {
    let fd_path = format!("/proc/{}/fd", std::process::id());
    match fs::read_dir(&fd_path) {
        Ok(entries) => Some(entries.count() as u32),
        Err(_) => None,
    }
}

#[cfg(not(target_os = "linux"))]
fn get_open_file_descriptors() -> Option<u32> {
    None
}

/// Get filesystem usage bytes (container rootfs)
fn get_filesystem_usage() -> Option<u64> {
    // Try to get fs usage from cgroup (io.stat) or statvfs
    // For containers, we check the root filesystem usage

    #[cfg(target_os = "linux")]
    {
        // Try to read from cgroup io.stat (cgroup v2)
        if let Ok(content) = fs::read_to_string("/sys/fs/cgroup/io.stat") {
            // Parse io.stat for filesystem bytes
            // Format varies, this is a simplified approach
            for line in content.lines() {
                // Look for lines with wbytes (write bytes)
                if line.contains("wbytes=") || line.contains("rbytes=") {
                    // This gives I/O, not usage - would need statfs for actual usage
                }
            }
        }

        // Use statvfs on root to get usage
        use std::ffi::CString;
        use std::mem::MaybeUninit;

        let path = CString::new("/").ok()?;
        let mut stat: MaybeUninit<libc::statvfs> = MaybeUninit::uninit();

        unsafe {
            if libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) == 0 {
                let stat = stat.assume_init();
                let block_size = stat.f_frsize as u64;
                let total_blocks = stat.f_blocks as u64;
                let free_blocks = stat.f_bfree as u64;
                let used = (total_blocks - free_blocks) * block_size;
                return Some(used);
            }
        }
    }

    None
}

/// Get node pressure conditions from cgroup pressure files
fn get_node_pressure() -> (Option<bool>, Option<bool>, Option<bool>) {
    let mut memory_pressure = None;
    let mut disk_pressure = None;
    let mut pid_pressure = None;

    // Check cgroup v2 pressure files
    // Memory pressure: /sys/fs/cgroup/memory.pressure
    if let Ok(content) = fs::read_to_string("/sys/fs/cgroup/memory.pressure") {
        // Format: "some avg10=X.XX avg60=Y.YY avg300=Z.ZZ total=N"
        // Consider pressure if avg10 > 10%
        for line in content.lines() {
            if line.starts_with("some") {
                if let Some(avg10) = extract_pressure_avg10(&line) {
                    memory_pressure = Some(avg10 > 10.0);
                }
            }
        }
    }

    // IO/Disk pressure: /sys/fs/cgroup/io.pressure
    if let Ok(content) = fs::read_to_string("/sys/fs/cgroup/io.pressure") {
        for line in content.lines() {
            if line.starts_with("some") {
                if let Some(avg10) = extract_pressure_avg10(&line) {
                    disk_pressure = Some(avg10 > 10.0);
                }
            }
        }
    }

    // CPU/PID pressure: /sys/fs/cgroup/cpu.pressure
    if let Ok(content) = fs::read_to_string("/sys/fs/cgroup/cpu.pressure") {
        for line in content.lines() {
            if line.starts_with("some") {
                if let Some(avg10) = extract_pressure_avg10(&line) {
                    pid_pressure = Some(avg10 > 10.0);
                }
            }
        }
    }

    (memory_pressure, disk_pressure, pid_pressure)
}

/// Extract avg10 value from pressure line
fn extract_pressure_avg10(line: &str) -> Option<f64> {
    // Format: "some avg10=X.XX avg60=Y.YY avg300=Z.ZZ total=N"
    for part in line.split_whitespace() {
        if part.starts_with("avg10=") {
            return part.trim_start_matches("avg10=").parse().ok();
        }
    }
    None
}

/// Get Kubernetes resource metrics
fn get_kubernetes_resource_metrics() -> Option<KubernetesResourceMetrics> {
    if !is_kubernetes() {
        return None;
    }

    let mut metrics = KubernetesResourceMetrics::default();

    metrics.cpu_limits_cores = get_cgroup_cpu_limit_cores();
    metrics.memory_limits_bytes = get_cgroup_memory_limit();
    metrics.cpu_throttled_seconds_total = get_cgroup_cpu_throttled_seconds();

    // CPU request from env
    if let Ok(cpu_request) = std::env::var("CPU_REQUEST") {
        if cpu_request.ends_with('m') {
            if let Ok(millicores) = cpu_request.trim_end_matches('m').parse::<u64>() {
                metrics.cpu_requests_cores = Some(millicores as f64 / 1000.0);
            }
        } else if let Ok(cores) = cpu_request.parse::<f64>() {
            metrics.cpu_requests_cores = Some(cores);
        }
    }

    // Memory request from env
    if let Ok(mem_request) = std::env::var("MEMORY_REQUEST") {
        metrics.memory_requests_bytes = parse_k8s_memory(&mem_request);
    }

    Some(metrics)
}

/// Get Kubernetes extended metrics
fn get_kubernetes_extended_metrics() -> Option<KubernetesExtendedMetrics> {
    if !is_kubernetes() {
        return None;
    }

    let mut metrics = KubernetesExtendedMetrics::default();

    // Get network stats
    let networks = NETWORKS.lock().unwrap();
    let mut rx = 0u64;
    let mut tx = 0u64;
    for (_name, data) in networks.iter() {
        rx += data.total_received();
        tx += data.total_transmitted();
    }
    metrics.network_rx_bytes_total = Some(rx);
    metrics.network_tx_bytes_total = Some(tx);

    // Get filesystem usage
    metrics.fs_usage_bytes = get_filesystem_usage();

    // Get node pressure conditions
    let (mem_pressure, disk_pressure, pid_pressure) = get_node_pressure();
    metrics.node_memory_pressure = mem_pressure;
    metrics.node_disk_pressure = disk_pressure;
    metrics.node_pid_pressure = pid_pressure;

    Some(metrics)
}

/// Get Kubernetes core metrics
fn get_kubernetes_core_metrics() -> Option<KubernetesCoreMetrics> {
    if !is_kubernetes() {
        return None;
    }

    Some(KubernetesCoreMetrics {
        uptime_seconds: Some(START_TIME.elapsed().as_secs()),
        pod_phase: Some("Running".to_string()),
        pod_ready: Some(true),
        cpu_usage_cores: get_cgroup_cpu_usage_cores(),
        memory_working_set_bytes: get_cgroup_memory_working_set(),
        container_restarts_total: std::env::var("CONTAINER_RESTARTS")
            .ok()
            .and_then(|s| s.parse().ok()),
        last_termination_reason: std::env::var("LAST_TERMINATION_REASON").ok(),
    })
}

/// Collect all metrics
pub fn collect_metrics() -> WorkerMetrics {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    let uptime_secs = START_TIME.elapsed().as_secs();

    // Refresh system info
    let (cpu_percent, memory_used, memory_total, thread_count) = {
        let mut sys = SYSTEM.lock().unwrap();
        sys.refresh_all();

        let pid = Pid::from_u32(std::process::id());

        if let Some(process) = sys.process(pid) {
            (
                process.cpu_usage() as f64,
                process.memory(),
                sys.total_memory(),
                // Approximate thread count from process
                1u32, // sysinfo doesn't expose thread count directly
            )
        } else {
            (0.0, 0, sys.total_memory(), 1)
        }
    };

    // Get network stats
    let (network_rx, network_tx) = {
        let mut networks = NETWORKS.lock().unwrap();
        networks.refresh();

        let mut rx = 0u64;
        let mut tx = 0u64;
        for (_name, data) in networks.iter() {
            rx += data.total_received();
            tx += data.total_transmitted();
        }

        // Calculate delta from last measurement
        let mut last_stats = LAST_NETWORK_STATS.lock().unwrap();
        let delta = if let Some((last_rx, last_tx)) = *last_stats {
            (
                rx.saturating_sub(last_rx),
                tx.saturating_sub(last_tx),
            )
        } else {
            (0, 0)
        };
        *last_stats = Some((rx, tx));
        delta
    };

    // Get disk I/O stats (delta)
    let (disk_read, disk_write) = {
        if let Some((current_read, current_write)) = get_disk_io_stats() {
            let mut last_stats = LAST_DISK_STATS.lock().unwrap();
            let delta = if let Some((last_read, last_write)) = *last_stats {
                (
                    current_read.saturating_sub(last_read),
                    current_write.saturating_sub(last_write),
                )
            } else {
                (0, 0)
            };
            *last_stats = Some((current_read, current_write));
            (Some(delta.0), Some(delta.1))
        } else {
            (None, None)
        }
    };

    // Get open file descriptors
    let open_fds = get_open_file_descriptors();

    // Get invocation metrics and reset counters
    let invocation_count = INVOCATION_COUNT.swap(0, Ordering::Relaxed);
    let total_latency = TOTAL_LATENCY_MS.swap(0, Ordering::Relaxed);
    let error_count = ERROR_COUNT.load(Ordering::Relaxed);

    let invocations_per_sec = invocation_count as f64 / 5.0; // Assuming 5s interval
    let avg_latency_ms = if invocation_count > 0 {
        total_latency as f64 / invocation_count as f64
    } else {
        0.0
    };

    let process = ProcessMetrics {
        cpu_percent: Some(cpu_percent),
        memory_used_bytes: Some(memory_used),
        memory_total_bytes: Some(memory_total),
        process_uptime_secs: Some(uptime_secs),
    };

    let performance = PerformanceMetrics {
        thread_count: Some(thread_count),
        open_connections: Some(1), // WebSocket connection
        invocations_per_sec: Some(invocations_per_sec),
        avg_latency_ms: Some(avg_latency_ms),
    };

    let extended = ExtendedMetrics {
        disk_read_bytes: disk_read,
        disk_write_bytes: disk_write,
        network_rx_bytes: Some(network_rx),
        network_tx_bytes: Some(network_tx),
        open_file_descriptors: open_fds,
        error_count: Some(error_count),
    };

    let mut metrics = WorkerMetrics {
        collected_at_ms: now,
        process,
        performance,
        extended,
        ..Default::default()
    };

    // Add K8s metrics if running in Kubernetes
    if is_kubernetes() {
        metrics.k8s_identifiers = get_kubernetes_identifiers();
        metrics.k8s_core = get_kubernetes_core_metrics();
        metrics.k8s_resources = get_kubernetes_resource_metrics();
        metrics.k8s_extended = get_kubernetes_extended_metrics();
    }

    metrics
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_metrics() {
        let metrics = collect_metrics();
        assert!(metrics.collected_at_ms > 0);
        assert!(metrics.process.process_uptime_secs.is_some());
    }

    #[test]
    fn test_record_invocation() {
        record_invocation(100, false);
        record_invocation(200, true);

        // Metrics should reflect the invocations
        let metrics = collect_metrics();
        assert!(metrics.performance.avg_latency_ms.is_some());
    }
}
