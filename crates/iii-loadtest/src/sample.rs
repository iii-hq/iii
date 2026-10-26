// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! What the runner reads from `/proc` about the engine and the CPUs it runs
//! on, one [`Sample`] per tick.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// One reading. CPU counters are cumulative, so usage over a window is the
/// difference between two samples divided by the difference of `t_ms`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sample {
    /// Milliseconds since sampling started.
    pub t_ms: f64,
    /// The engine's user + system CPU time, in milliseconds.
    pub proc_cpu_ms: f64,
    pub rss_kb: u64,
    /// Peak RSS the kernel has seen for the engine (`VmHWM`).
    pub hwm_kb: u64,
    pub threads: u64,
    /// The engine's CPUs, busy time (anything but idle and iowait) in
    /// milliseconds: the engine plus the kernel work done on them (network
    /// softirqs, page faults), and anything else scheduled there.
    pub cpus_busy_ms: f64,
    pub cpus_total_ms: f64,
    pub mem_total_kb: u64,
    pub mem_available_kb: u64,
    /// The engine's cgroup, when it has one of its own: everything charged
    /// to it (page cache included), its peak, and OOM kills.
    pub cgroup_current: Option<u64>,
    pub cgroup_peak: Option<u64>,
    pub cgroup_oom_kills: Option<u64>,
    /// This process's CPU time (the load generator), in milliseconds.
    pub loadgen_cpu_ms: f64,
}

fn clk_tck() -> f64 {
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if ticks > 0 { ticks as f64 } else { 100.0 }
}

/// `utime + stime` of `/proc/<pid>/stat`, in milliseconds. The fields are
/// counted after the command name, which may itself hold spaces.
pub fn parse_proc_cpu_ms(stat: &str) -> Option<f64> {
    let rest = &stat[stat.rfind(')')? + 2..];
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // After `)`: state is field 3, utime 14 and stime 15 (1-based).
    let utime: f64 = fields.get(11)?.parse().ok()?;
    let stime: f64 = fields.get(12)?.parse().ok()?;
    Some((utime + stime) * 1000.0 / clk_tck())
}

/// The process state of `/proc/<pid>/stat` (after the command name) is
/// zombie or dead.
fn is_dead(stat: &str) -> bool {
    stat.rfind(')')
        .and_then(|i| stat[i + 1..].split_whitespace().next())
        .is_some_and(|state| state == "Z" || state == "X")
}

fn status_kb(status: &str, key: &str) -> u64 {
    status
        .lines()
        .find(|l| l.starts_with(key))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// Busy and total milliseconds of the `cpuN` lines of `/proc/stat` for
/// `cpus`.
fn parse_cpus_ms(stat: &str, cpus: &[usize]) -> (f64, f64) {
    let (mut busy, mut total) = (0.0, 0.0);
    for line in stat.lines() {
        let Some(rest) = line.strip_prefix("cpu") else {
            continue;
        };
        let mut fields = rest.split_whitespace();
        let Some(Ok(n)) = fields.next().map(str::parse::<usize>) else {
            continue;
        };
        if !cpus.contains(&n) {
            continue;
        }
        // user nice system idle iowait irq softirq steal (guest time is in
        // user). Busy is summed from the busy fields, not total minus idle:
        // iowait can go backwards (it does in the guest), the others cannot.
        let v: Vec<f64> = fields.take(8).filter_map(|x| x.parse().ok()).collect();
        let field = |i: usize| v.get(i).copied().unwrap_or(0.0);
        busy += field(0) + field(1) + field(2) + field(5) + field(6) + field(7);
        total += v.iter().sum::<f64>();
    }
    let scale = 1000.0 / clk_tck();
    (busy * scale, total * scale)
}

fn read_u64(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

pub fn oom_kills(cgroup: &Path) -> Option<u64> {
    let events = std::fs::read_to_string(cgroup.join("memory.events")).ok()?;
    events
        .lines()
        .find_map(|l| l.strip_prefix("oom_kill "))
        .and_then(|v| v.trim().parse().ok())
}

/// Reads the engine with `pid`, the CPUs it is pinned to, and its cgroup.
/// `None` once the process is gone.
pub fn read(pid: u32, t_ms: f64, cpus: &[usize], cgroup: Option<&Path>) -> Option<Sample> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // A dead engine its parent has not reaped yet is still in /proc, as a
    // zombie: it is gone all the same.
    if is_dead(&stat) {
        return None;
    }
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let sys = std::fs::read_to_string("/proc/stat").unwrap_or_default();
    let meminfo = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let (cpus_busy_ms, cpus_total_ms) = parse_cpus_ms(&sys, cpus);
    Some(Sample {
        t_ms,
        proc_cpu_ms: parse_proc_cpu_ms(&stat)?,
        rss_kb: status_kb(&status, "VmRSS:"),
        hwm_kb: status_kb(&status, "VmHWM:"),
        threads: status_kb(&status, "Threads:"),
        cpus_busy_ms,
        cpus_total_ms,
        mem_total_kb: status_kb(&meminfo, "MemTotal:"),
        mem_available_kb: status_kb(&meminfo, "MemAvailable:"),
        cgroup_current: cgroup.and_then(|c| read_u64(&c.join("memory.current"))),
        cgroup_peak: cgroup.and_then(|c| read_u64(&c.join("memory.peak"))),
        cgroup_oom_kills: cgroup.and_then(oom_kills),
        loadgen_cpu_ms: self_cpu_ms(),
    })
}

/// CPU time this process has used so far, user + system, in milliseconds.
pub fn self_cpu_ms() -> f64 {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    let ms = |tv: libc::timeval| tv.tv_sec as f64 * 1000.0 + tv.tv_usec as f64 / 1000.0;
    ms(usage.ru_utime) + ms(usage.ru_stime)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proc_cpu_survives_spaces_in_the_command_name() {
        let stat = "42 (iii engine) S 1 42 42 0 -1 4194560 100 0 0 0 250 50 0 0 20 0 9 0 1 1 1";
        let ms = parse_proc_cpu_ms(stat).unwrap();
        assert!((ms - 300.0 * 1000.0 / clk_tck()).abs() < 1e-6);
    }

    #[test]
    fn only_the_engines_cpus_count_and_idle_is_not_busy() {
        let stat = "cpu  300 0 150 2400 150 0 0 0 0 0\n\
                    cpu0 100 0 50 800 50 0 0 0 0 0\n\
                    cpu1 100 0 50 800 50 0 0 0 0 0\n\
                    cpu2 100 0 50 800 50 0 0 0 0 0\n";
        let (busy, total) = parse_cpus_ms(stat, &[0, 2]);
        let scale = 1000.0 / clk_tck();
        assert_eq!(busy, 300.0 * scale);
        assert_eq!(total, 2000.0 * scale);
    }

    #[test]
    fn a_zombie_is_dead() {
        assert!(is_dead("42 (iii) Z 1 42 42 0"));
        assert!(is_dead("42 (iii (x)) X 1"));
        assert!(!is_dead("42 (iii) S 1 42 42 0"));
        assert!(!is_dead("42 (Z) R 1"));
    }

    #[test]
    fn status_values_are_read_in_kb() {
        let status = "Name:\tiii\nVmHWM:\t  2048 kB\nVmRSS:\t  1024 kB\nThreads:\t9\n";
        assert_eq!(status_kb(status, "VmRSS:"), 1024);
        assert_eq!(status_kb(status, "VmHWM:"), 2048);
        assert_eq!(status_kb(status, "Threads:"), 9);
        assert_eq!(status_kb(status, "Missing:"), 0);
    }
}
