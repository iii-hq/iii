use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use sysinfo::{Disks, Networks, Pid, System};

use crate::bridge::{
    ExtendedMetrics, KubernetesCoreMetrics, KubernetesExtendedMetrics, KubernetesIdentifiers,
    KubernetesResourceMetrics, PerformanceMetrics, ProcessMetrics, WorkerMetrics,
};

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

pub fn record_invocation(latency_ms: u64, is_error: bool) {
    INVOCATION_COUNT.fetch_add(1, Ordering::Relaxed);
    TOTAL_LATENCY_MS.fetch_add(latency_ms, Ordering::Relaxed);
    if is_error {
        ERROR_COUNT.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn is_kubernetes() -> bool {
    std::env::var("KUBERNETES_SERVICE_HOST").is_ok()
}

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

fn read_k8s_file(filename: &str) -> Option<String> {
    let path = format!("/var/run/secrets/kubernetes.io/serviceaccount/{}", filename);
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

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

fn get_cgroup_cpu_usage_cores() -> Option<f64> {
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

    if let Ok(content) = fs::read_to_string("/sys/fs/cgroup/cpu/cpuacct.usage") {
        if let Ok(nsec) = content.trim().parse::<u64>() {
            return Some(nsec as f64 / 1_000_000_000.0);
        }
    }

    None
}

fn get_cgroup_memory_working_set() -> Option<u64> {
    read_cgroup_value(
        "/sys/fs/cgroup/memory/memory.usage_in_bytes",
        "/sys/fs/cgroup/memory.current",
    )
}

fn get_cgroup_memory_limit() -> Option<u64> {
    let value = read_cgroup_value(
        "/sys/fs/cgroup/memory/memory.limit_in_bytes",
        "/sys/fs/cgroup/memory.max",
    )?;

    if value < 9223372036854771712 {
        Some(value)
    } else {
        None
    }
}

fn get_cgroup_cpu_limit_cores() -> Option<f64> {
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

fn get_cgroup_cpu_throttled_seconds() -> Option<f64> {
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

fn parse_k8s_memory(value: &str) -> Option<u64> {
    let value = value.trim();

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

#[cfg(target_os = "linux")]
fn get_disk_io_stats() -> Option<(u64, u64)> {
    let content = fs::read_to_string("/proc/diskstats").ok()?;

    let mut total_read = 0u64;
    let mut total_write = 0u64;

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 14 {
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

fn get_filesystem_usage() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = fs::read_to_string("/sys/fs/cgroup/io.stat") {
            for line in content.lines() {
                if line.contains("wbytes=") || line.contains("rbytes=") {
                }
            }
        }

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

fn get_node_pressure() -> (Option<bool>, Option<bool>, Option<bool>) {
    let mut memory_pressure = None;
    let mut disk_pressure = None;
    let mut pid_pressure = None;

    if let Ok(content) = fs::read_to_string("/sys/fs/cgroup/memory.pressure") {
        for line in content.lines() {
            if line.starts_with("some") {
                if let Some(avg10) = extract_pressure_avg10(&line) {
                    memory_pressure = Some(avg10 > 10.0);
                }
            }
        }
    }

    if let Ok(content) = fs::read_to_string("/sys/fs/cgroup/io.pressure") {
        for line in content.lines() {
            if line.starts_with("some") {
                if let Some(avg10) = extract_pressure_avg10(&line) {
                    disk_pressure = Some(avg10 > 10.0);
                }
            }
        }
    }

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

fn extract_pressure_avg10(line: &str) -> Option<f64> {
    for part in line.split_whitespace() {
        if part.starts_with("avg10=") {
            return part.trim_start_matches("avg10=").parse().ok();
        }
    }
    None
}

fn get_kubernetes_resource_metrics() -> Option<KubernetesResourceMetrics> {
    if !is_kubernetes() {
        return None;
    }

    let mut metrics = KubernetesResourceMetrics::default();

    metrics.cpu_limits_cores = get_cgroup_cpu_limit_cores();
    metrics.memory_limits_bytes = get_cgroup_memory_limit();
    metrics.cpu_throttled_seconds_total = get_cgroup_cpu_throttled_seconds();

    if let Ok(cpu_request) = std::env::var("CPU_REQUEST") {
        if cpu_request.ends_with('m') {
            if let Ok(millicores) = cpu_request.trim_end_matches('m').parse::<u64>() {
                metrics.cpu_requests_cores = Some(millicores as f64 / 1000.0);
            }
        } else if let Ok(cores) = cpu_request.parse::<f64>() {
            metrics.cpu_requests_cores = Some(cores);
        }
    }

    if let Ok(mem_request) = std::env::var("MEMORY_REQUEST") {
        metrics.memory_requests_bytes = parse_k8s_memory(&mem_request);
    }

    Some(metrics)
}

fn get_kubernetes_extended_metrics() -> Option<KubernetesExtendedMetrics> {
    if !is_kubernetes() {
        return None;
    }

    let mut metrics = KubernetesExtendedMetrics::default();

    let networks = NETWORKS.lock().unwrap();
    let mut rx = 0u64;
    let mut tx = 0u64;
    for (_name, data) in networks.iter() {
        rx += data.total_received();
        tx += data.total_transmitted();
    }
    metrics.network_rx_bytes_total = Some(rx);
    metrics.network_tx_bytes_total = Some(tx);

    metrics.fs_usage_bytes = get_filesystem_usage();

    let (mem_pressure, disk_pressure, pid_pressure) = get_node_pressure();
    metrics.node_memory_pressure = mem_pressure;
    metrics.node_disk_pressure = disk_pressure;
    metrics.node_pid_pressure = pid_pressure;

    Some(metrics)
}

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

pub fn collect_metrics() -> WorkerMetrics {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    let uptime_secs = START_TIME.elapsed().as_secs();

    let (cpu_percent, memory_used, memory_total, thread_count) = {
        let mut sys = SYSTEM.lock().unwrap();
        sys.refresh_all();

        let pid = Pid::from_u32(std::process::id());

        if let Some(process) = sys.process(pid) {
            (
                process.cpu_usage() as f64,
                process.memory(),
                sys.total_memory(),
                1u32,
            )
        } else {
            (0.0, 0, sys.total_memory(), 1)
        }
    };

    let (network_rx, network_tx) = {
        let mut networks = NETWORKS.lock().unwrap();
        networks.refresh();

        let mut rx = 0u64;
        let mut tx = 0u64;
        for (_name, data) in networks.iter() {
            rx += data.total_received();
            tx += data.total_transmitted();
        }

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

    let open_fds = get_open_file_descriptors();

    let invocation_count = INVOCATION_COUNT.swap(0, Ordering::Relaxed);
    let total_latency = TOTAL_LATENCY_MS.swap(0, Ordering::Relaxed);
    let error_count = ERROR_COUNT.load(Ordering::Relaxed);

    let invocations_per_sec = invocation_count as f64 / 5.0;
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
        open_connections: Some(1),
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

        let metrics = collect_metrics();
        assert!(metrics.performance.avg_latency_ms.is_some());
    }
}
